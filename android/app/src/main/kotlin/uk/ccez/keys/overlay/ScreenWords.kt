package uk.ccez.keys.overlay

import android.content.Context
import android.graphics.PixelFormat
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.view.GestureDetector
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.TextView
import kotlinx.coroutines.launch
import uk.ccez.keys.App
import uk.ccez.keys.Colors
import uk.ccez.keys.core.Grade
import uk.ccez.keys.core.LearningWord
import uk.ccez.keys.core.ScreenWord

/**
 * The words themselves: each its own small overlay window over every app,
 * as the Mac's floaters (desktop/src/float.rs). Faint until touched; a
 * touch shows the meaning under the word without moving it. Tap: Good,
 * double-tap: Easy, long-press: Again (meaning shown a moment). Each
 * leaves after 90 s, or at once when its spot fills up. Touches elsewhere
 * reach the app below (each window is only as big as its word).
 */
class ScreenWords(private val ctx: Context) {
    private val wm = ctx.getSystemService(WindowManager::class.java)
    private val main = Handler(Looper.getMainLooper())
    private val density = ctx.resources.displayMetrics.density

    private class Up(
        val word: ScreenWord,
        val cell: FreeSpace.Box,
        val view: LinearLayout,
        val params: WindowManager.LayoutParams,
        val meaningRow: LinearLayout,
        val came: Long,
    ) {
        var touching = false
        var graded = false
        var open = false
        var closedWidth = 0
    }

    private val up = ArrayList<Up>()
    private var lastAdd = 0L
    private var fetching = false

    private fun dp(v: Float) = (v * density).toInt()

    private fun count() = App.instance.number("learning.screen")

    /** A fresh look at the free spots (every 4 s): crowded and old words go, a new one may come. Main thread. */
    fun onSpots(cells: List<FreeSpace.Box>) {
        val most = count()
        if (most <= 0 || !Settings.canDrawOverlays(ctx)) {
            clear(now = false)
            return
        }
        val now = System.currentTimeMillis()
        val gone = up.filter { w -> (!w.touching && now - w.came > STAYS_MS) || cells.none { it == w.cell } }
        gone.forEach { leave(it) }
        // Fewer asked for in Settings: the newest go.
        while (up.size > most) leave(up.last())
        if (up.size >= most || fetching || now - lastAdd < EVERY_MS) return
        val cell = cells.firstOrNull { c -> up.none { it.cell == c } } ?: return
        fetching = true
        val showing = up.map { LearningWord(it.word.lang, it.word.word) }
        App.instance.scope.launch {
            val fresh = runCatching { App.instance.engine.screenWords(showing) }.getOrDefault(emptyList())
            main.post {
                fetching = false
                val pick = fresh.firstOrNull() ?: return@post
                if (count() <= 0 || up.size >= count() || up.any { it.word.lang == pick.lang && it.word.word == pick.word }) return@post
                lastAdd = System.currentTimeMillis()
                show(pick, cell)
            }
        }
    }

    /** Every word away at once, no fade (a screenshot, the lock screen, the screen going off). Main thread. */
    fun hideNow() = clear(now = true)

    /** Every word away (switched off, a password field, a video with the switch off). Main thread. */
    fun hide() = clear(now = false)

    private fun clear(now: Boolean) {
        val all = up.toList()
        up.clear()
        all.forEach { remove(it, now) }
    }

    private fun leave(w: Up) {
        up.remove(w)
        remove(w, now = false)
    }

    private fun remove(w: Up, now: Boolean) {
        if (now) {
            w.view.animate().cancel()
            runCatching { wm.removeView(w.view) }
        } else {
            // Never fade under a finger: a held word goes when it's let go.
            w.view.animate().alpha(0f).setDuration(FADE_OUT_MS).withEndAction { runCatching { wm.removeView(w.view) } }.start()
        }
    }

    private fun grade(w: Up, g: Grade) {
        if (w.graded) return
        w.graded = true
        App.instance.scope.launch {
            runCatching { App.instance.engine.gradeScreenWord(w.word.lang, w.word.word, g) }
            uk.ccez.keys.learn.WordSync.soon()
        }
    }

    private fun show(word: ScreenWord, cell: FreeSpace.Box) {
        val view = LinearLayout(ctx).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            background = GradientDrawable().apply {
                setColor(0xB81A1A1C.toInt())
                cornerRadius = dp(16f).toFloat()
            }
            setPadding(dp(12f), dp(6f), dp(12f), dp(7f))
        }
        view.addView(
            TextView(ctx).apply {
                text = word.shown
                textSize = 20f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(Colors.ACCENT)
                gravity = Gravity.CENTER
            },
        )
        val meaningRow = LinearLayout(ctx).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            visibility = View.GONE
        }
        if (word.meaning.isNotEmpty()) {
            meaningRow.addView(
                TextView(ctx).apply {
                    text = word.meaning
                    textSize = 13f
                    setTextColor(Colors.FG)
                },
            )
        }
        word.tags.forEach { tag ->
            meaningRow.addView(
                TextView(ctx).apply {
                    text = tag
                    textSize = 10.5f
                    setTextColor(Colors.FG)
                    background = GradientDrawable().apply {
                        setColor(0x1FFFFFFF)
                        cornerRadius = dp(8f).toFloat()
                    }
                    setPadding(dp(5f), dp(1f), dp(5f), dp(1f))
                    (layoutParams as? LinearLayout.LayoutParams)?.marginStart = dp(4f)
                },
            )
        }
        view.addView(meaningRow)
        view.measure(View.MeasureSpec.UNSPECIFIED, View.MeasureSpec.UNSPECIFIED)
        val params = WindowManager.LayoutParams(
            view.measuredWidth.coerceAtMost(cell.width()),
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            // Never in a screenshot or a screen recording, where the
            // per-window FLAG_SECURE is honoured for overlays; hiding when
            // the screenshot UI appears (the service) is the second belt.
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL or
                WindowManager.LayoutParams.FLAG_SECURE,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            x = (cell.centerX() - view.measuredWidth / 2).coerceAtLeast(0)
            y = (cell.centerY() - view.measuredHeight / 2).coerceAtLeast(0)
        }
        val w = Up(word, cell, view, params, meaningRow, System.currentTimeMillis()).apply { closedWidth = view.measuredWidth }
        val detector = GestureDetector(
            ctx,
            object : GestureDetector.SimpleOnGestureListener() {
                override fun onDown(e: MotionEvent): Boolean {
                    w.touching = true
                    open(w)
                    return true
                }

                override fun onSingleTapConfirmed(e: MotionEvent): Boolean {
                    grade(w, Grade.GOOD)
                    leave(w)
                    return true
                }

                override fun onDoubleTap(e: MotionEvent): Boolean {
                    grade(w, Grade.EASY)
                    leave(w)
                    return true
                }

                override fun onLongPress(e: MotionEvent) {
                    grade(w, Grade.AGAIN)
                    open(w)
                    // Its meaning a moment, then away.
                    main.postDelayed({ if (w in up) leave(w) }, AGAIN_SHOW_MS)
                }
            },
        )
        view.setOnTouchListener { _, e ->
            detector.onTouchEvent(e)
            if (e.actionMasked == MotionEvent.ACTION_UP || e.actionMasked == MotionEvent.ACTION_CANCEL) w.touching = false
            true
        }
        view.alpha = 0f
        runCatching { wm.addView(view, params) }.onFailure {
            App.instance.log("warn", "screen-word-failed", "error" to it.javaClass.simpleName)
            return
        }
        up += w
        view.animate().alpha(FAINT).setDuration(FADE_IN_MS).start()
    }

    /** Touched: lit, its meaning under it. The window grows downward; the word itself doesn't move. */
    private fun open(w: Up) {
        w.view.animate().cancel()
        w.view.animate().alpha(1f).setDuration(150).start()
        if (w.open || (w.word.meaning.isEmpty() && w.word.tags.isEmpty())) return
        w.open = true
        w.meaningRow.visibility = View.VISIBLE
        w.view.measure(View.MeasureSpec.UNSPECIFIED, View.MeasureSpec.UNSPECIFIED)
        val newWidth = w.view.measuredWidth
        w.params.x = (w.params.x - (newWidth - w.closedWidth) / 2).coerceAtLeast(0)
        w.params.width = newWidth
        runCatching { wm.updateViewLayout(w.view, w.params) }
    }

    /** Where the words' own windows are, so the spot look doesn't count a word as content crowding itself out. Main thread. */
    fun ownWindows(): List<FreeSpace.Box> = up.map {
        FreeSpace.Box(it.params.x, it.params.y, it.params.x + it.view.width, it.params.y + it.view.height)
    }

    /** For `keys screen` (checks): how many are up, and which. Main thread not needed. */
    fun status(): String {
        val o = org.json.JSONObject()
            .put("overlay", Settings.canDrawOverlays(ctx))
            .put("count", count())
            .put("up", up.size)
        val words = org.json.JSONArray()
        synchronized(up) { up.forEach { words.put("${it.word.lang}:${it.word.word}") } }
        return o.put("words", words).toString()
    }

    companion object {
        /** How long a word stays when left alone. */
        const val STAYS_MS = 90_000L
        /** A new word at most this often (the spot look's own pace). */
        const val EVERY_MS = 4_000L
        /** Fading in this slowly, they don't pull the eye. */
        const val FADE_IN_MS = 2_000L
        const val FADE_OUT_MS = 800L
        const val AGAIN_SHOW_MS = 2_500L
        /** Faint until touched. */
        const val FAINT = 0.75f
    }
}
