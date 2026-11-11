package uk.ccez.keys.overlay

import android.accessibilityservice.AccessibilityService
import android.app.KeyguardManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.Rect
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.provider.Settings
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import uk.ccez.keys.App

/**
 * Free space inside whatever app is in front, for the words on screen
 * ([ScreenWords]): the bounds of the active windows' content (text,
 * images and controls, by class and flags — their text is never read),
 * looked at most every 4 s and on window changes, never while the screen
 * is off. Off on the lock screen and in password fields; off in a
 * full-screen video or with the keyboard up when their switches say so.
 * When the screenshot UI appears the words hide at once, so they never
 * land in a screenshot (an overlay window can't be kept out of one any
 * other way: FLAG_SECURE is not per window for overlays).
 */
class ScreenWordsService : AccessibilityService() {
    private val main = Handler(Looper.getMainLooper())
    private lateinit var words: ScreenWords
    private var lastScan = 0L
    private var scanPosted = false
    private var running = false

    private val screen = object : BroadcastReceiver() {
        override fun onReceive(c: Context?, i: Intent?) {
            when (i?.action) {
                Intent.ACTION_SCREEN_OFF -> {
                    running = false
                    main.removeCallbacks(tick)
                    words.hideNow()
                }
                Intent.ACTION_SCREEN_ON, Intent.ACTION_USER_PRESENT -> {
                    if (!running) {
                        running = true
                        main.post(tick)
                    }
                }
            }
        }
    }

    private val tick = object : Runnable {
        override fun run() {
            scan()
            if (running) main.postDelayed(this, EVERY_MS)
        }
    }

    override fun onServiceConnected() {
        words = ScreenWords(applicationContext)
        App.instance.screenWords = words
        registerReceiver(screen, IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_OFF)
            addAction(Intent.ACTION_SCREEN_ON)
            addAction(Intent.ACTION_USER_PRESENT)
        })
        val pm = getSystemService(PowerManager::class.java)
        running = pm?.isInteractive == true
        if (running) main.post(tick)
        App.instance.log("info", "screen-words-service")
    }

    override fun onUnbind(intent: Intent?): Boolean {
        running = false
        main.removeCallbacks(tick)
        runCatching { unregisterReceiver(screen) }
        if (::words.isInitialized) words.hideNow()
        App.instance.screenWords = null
        return super.onUnbind(intent)
    }

    override fun onInterrupt() = Unit

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        event ?: return
        // The screenshot panel arriving: words off the screen at once.
        if (event.eventType == AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED &&
            FreeSpace.isScreenshotWindow(event.packageName, null, event.className, emptyList())
        ) {
            if (::words.isInitialized) words.hideNow()
        }
        if (!running) return
        // Look again, at most every 4 s however busy the window is.
        val wait = EVERY_MS - (System.currentTimeMillis() - lastScan)
        if (wait <= 0) {
            scan()
        } else if (!scanPosted) {
            scanPosted = true
            main.postDelayed({ scanPosted = false; scan() }, wait)
        }
    }

    private fun scan() {
        if (!::words.isInitialized) return
        lastScan = System.currentTimeMillis()
        val app = App.instance
        if (app.number("learning.screen") <= 0 || !Settings.canDrawOverlays(this)) {
            words.hide()
            return
        }
        if (getSystemService(KeyguardManager::class.java)?.isKeyguardLocked == true) {
            words.hideNow()
            return
        }
        val metrics = resources.displayMetrics
        val screenRect = FreeSpace.Box(0, 0, metrics.widthPixels, metrics.heightPixels)
        val blockers = ArrayList<FreeSpace.Box>()
        var keyboardUp = false
        var videoFull = false
        var password = false
        var screenshot = false
        for (window in windows.orEmpty()) {
            if (window.type == AccessibilityWindowInfo.TYPE_INPUT_METHOD) {
                keyboardUp = true
                continue
            }
            if (window.type != AccessibilityWindowInfo.TYPE_APPLICATION && window.type != AccessibilityWindowInfo.TYPE_SYSTEM) continue
            val root = window.root ?: continue
            val pkg = root.packageName?.toString().orEmpty()
            if (pkg == packageName) {
                root.recycle()
                continue
            }
            if (FreeSpace.isScreenshotWindow(pkg, window.title, null, emptyList())) screenshot = true
            if (window.type != AccessibilityWindowInfo.TYPE_APPLICATION) {
                root.recycle()
                continue
            }
            val windowBounds = Rect()
            window.getBoundsInScreen(windowBounds)
            val windowArea = (windowBounds.width().toLong() * windowBounds.height()).coerceAtLeast(1)
            // Bounds only, never text: a node counts by its kind.
            val queue = ArrayDeque<AccessibilityNodeInfo>()
            queue.add(root)
            var seen = 0
            while (queue.isNotEmpty() && seen < MAX_NODES) {
                val node = queue.removeFirst()
                seen++
                for (i in 0 until node.childCount) node.getChild(i)?.let { queue.add(it) }
                if (node.isVisibleToUser) {
                    if (node.isPassword) password = true
                    val b = Rect()
                    node.getBoundsInScreen(b)
                    val area = b.width().toLong() * b.height()
                    if (area > 0) {
                        if (FreeSpace.isVideoClass(node.className) && area > screenRect.width().toLong() * screenRect.height() * 3 / 5) videoFull = true
                        val content = FreeSpace.isContentClass(node.className) || node.isClickable || node.isEditable || node.isCheckable
                        // A big empty container is a backdrop, not content (as on the Mac).
                        if (content && area < windowArea * FreeSpace.BIG) blockers += FreeSpace.Box(b.left, b.top, b.right, b.bottom)
                        if (pkg == "com.android.systemui" && node.viewIdResourceName?.contains("screenshot", ignoreCase = true) == true) screenshot = true
                    }
                }
                node.recycle()
            }
            while (queue.isNotEmpty()) queue.removeFirst().recycle()
        }
        when {
            screenshot -> words.hideNow()
            password -> words.hide()
            keyboardUp && !app.flag("learning.screen_typing") -> words.hide()
            videoFull && !app.flag("learning.screen_video") -> words.hide()
            else -> words.onSpots(FreeSpace.freeCells(screenRect, blockers, COLS, ROWS))
        }
    }

    companion object {
        /** Spots looked for this often (and on window changes, no oftener). */
        const val EVERY_MS = 4_000L
        private const val COLS = 3
        private const val ROWS = 7
        private const val MAX_NODES = 4000

        /** This service on? (Settings → Accessibility; sideloaded apps need restricted settings allowed first.) */
        fun enabled(ctx: Context): Boolean {
            val want = "${ctx.packageName}/${ScreenWordsService::class.java.name}"
            val on = Settings.Secure.getString(ctx.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES).orEmpty()
            return on.split(':').any { it.equals(want, ignoreCase = true) }
        }
    }
}
