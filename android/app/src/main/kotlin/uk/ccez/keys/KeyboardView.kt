package uk.ccez.keys

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.os.Handler
import android.os.Looper
import android.view.MotionEvent
import android.view.View
import uk.ccez.keys.core.Key
import uk.ccez.keys.core.KeyKind
import uk.ccez.keys.core.Layout
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

enum class Shift { OFF, ONCE, LOCK }

/** How long delete is held before it takes whole words (ms). */
/**
 * Held delete, as SwiftKey does it (measured on the owner's S24): nothing
 * letter by letter; a whole word once held 0.45 s, the next 0.5 s later,
 * then one every 0.25 s, quickening from the fifth word to one every
 * 0.12 s.
 */
const val WORD_HOLD_MS = 450L
const val WORD_SECOND_MS = 500L

/** The wait before the next word, after `n` words gone. */
fun wordGap(n: Int): Long = if (n == 1) WORD_SECOND_MS else (250L - 30L * maxOf(0, n - 4)).coerceIn(120L, 250L)

/** A held key's choices, in this view's coordinates. */
data class Popup(val key: RectF, val box: RectF, val choices: List<String>, val selected: Int, val codes: List<String> = emptyList(), val cols: Int = choices.size)

/**
 * The keys, drawn on one canvas so a press shows in the same frame. All
 * it decides is which key was touched and how (tap, hold, slide on the
 * space bar); what that means for the text is Typist's job.
 */
@SuppressLint("ViewConstructor")
class KeyboardView(context: Context, private val listener: Listener) : View(context) {
    interface Listener {
        fun onKey(key: Key)
        fun onChoice(text: String)
        fun onSpaceSwipe(step: Int)
        fun onPress(key: Key)
        fun onPopup(popup: Popup?)
        /** A language picked from the space bar's list. */
        fun onLanguage(code: String)
        /** Delete held, for `heldMs` so far. */
        fun onRepeat(key: Key, heldMs: Long)
        /** ?123 held (Japanese, Chinese: draw a character). */
        fun onHoldSymbols()
        /** A swipe across the letters (the ones passed, in order). */
        fun onSwipe(path: String)
    }

    var layout: Layout? = null
        set(v) { field = v; requestLayout(); placeKeys(); invalidate() }
    var shift = Shift.OFF
        set(v) { if (field != v) { field = v; invalidate() } }
    var spaceLabel = ""
        set(v) { if (field != v) { field = v; invalidate() } }
    /** The language was just picked by what was typed: a dot after its name, gone after a second. */
    var auto = false
        set(v) {
            field = v
            removeCallbacks(hideDot)
            if (v) postDelayed(hideDot, 1000)
            invalidate()
        }
    private val hideDot = Runnable { auto = false }
    var showArrows = false
        set(v) { if (field != v) { field = v; invalidate() } }
    /** Swipe typing (Settings). */
    var swipe = false
    private var swiping = false
    private val trail = android.graphics.Path()
    private var swipePath = StringBuilder()

    /** The space bar's list: (code, flag), and the one in use. */
    var languages: List<Pair<String, String>> = emptyList()
    var language = ""
    var scale = 1f
        set(v) { if (field != v) { field = v; requestLayout() } }

    private val dp = resources.displayMetrics.density
    private val sp = resources.displayMetrics.scaledDensity
    private val rowH get() = 44 * dp * scale
    private val rowGap = 8 * dp
    private val padTop = 6 * dp
    private val padBottom = 6 * dp
    private val side = 3 * dp
    private val gap = 5 * dp

    private class Placed(val key: Key, val cell: RectF, val face: RectF)
    private var placed: List<Placed> = emptyList()

    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val text = Paint(Paint.ANTI_ALIAS_FLAG).apply { textAlign = Paint.Align.CENTER }
    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE; strokeWidth = 1.7f * dp; strokeCap = Paint.Cap.ROUND }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val rows = layout?.rows?.size ?: 4
        val h = padTop + padBottom + rows * rowH + (rows - 1) * rowGap
        setMeasuredDimension(MeasureSpec.getSize(widthMeasureSpec), h.toInt())
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) = placeKeys()

    private fun placeKeys() {
        val l = layout ?: return
        if (width == 0) return
        // Ten keys a row on QWERTY; scripts with 11 or 12 letters a row get narrower keys.
        val widest = l.rows.filter { r -> r.keys.all { it.kind == KeyKind.CHAR } }.maxOfOrNull { r -> r.keys.sumOf { it.width.toDouble() } }?.toFloat() ?: 10f
        val span = maxOf(10f, widest)
        val unit = (width - 2 * side) / span
        val out = ArrayList<Placed>()
        l.rows.forEachIndexed { r, row ->
            val units = row.keys.sumOf { it.width.toDouble() }.toFloat()
            val onlyChars = row.keys.all { it.kind == KeyKind.CHAR }
            // A short row of letters sits centred; any row with other keys spans the width.
            val u = if (onlyChars) unit else (width - 2 * side) / units
            var x = side + if (onlyChars) (span - units) * unit / 2 else 0f
            val top = padTop + r * (rowH + rowGap)
            row.keys.forEach { k ->
                val w = k.width * u
                val cell = RectF(x, top - rowGap / 2, x + w, top + rowH + rowGap / 2)
                val face = RectF(x + gap / 2, top, x + w - gap / 2, top + rowH)
                // Shift and delete keep their touch area but look narrower,
                // with room on the letters' side (z and m stay put).
                if (k.width > 1.2f && row.keys.any { it.kind == KeyKind.CHAR }) {
                    val trim = 0.22f * unit
                    if (k.kind == KeyKind.SHIFT || k.kind == KeyKind.MORE || (k.kind == KeyKind.SYMBOLS && r < l.rows.size - 1)) face.right -= trim
                    if (k.kind == KeyKind.BACKSPACE) face.left += trim
                }
                out += Placed(k, cell, face)
                x += w
            }
        }
        // Edge keys reach the edges, so a touch at the very side still lands.
        placed = out
    }

    override fun onDraw(c: Canvas) {
        // A rect, not drawColor: drawColor ignores this view's bounds and
        // painted over the bar above it.
        fill.color = Colors.KB_BG
        c.drawRect(0f, 0f, width.toFloat(), height.toFloat(), fill)
        val r = 6 * dp
        for (p in placed) {
            val k = p.key
            fill.color = when {
                p === pressed -> Colors.KEY_PRESSED
                k.kind == KeyKind.ENTER -> Colors.ENTER
                k.kind == KeyKind.CHAR || k.kind == KeyKind.SPACE -> if (k.kind == KeyKind.CHAR && k.width < 1f) Colors.MOD else Colors.KEY
                k.kind == KeyKind.SHIFT && shift == Shift.LOCK -> Colors.FACE_ON
                else -> Colors.MOD
            }
            c.drawRoundRect(p.face, r, r, fill)
            drawLabel(c, p)
        }
        if (swiping) {
            stroke.color = Colors.ACCENT
            stroke.strokeWidth = 4 * dp
            c.drawPath(trail, stroke)
            stroke.strokeWidth = 1.7f * dp
        }
    }

    private fun drawLabel(c: Canvas, p: Placed) {
        val k = p.key
        val cx = p.face.centerX()
        val cy = p.face.centerY()
        when (k.kind) {
            KeyKind.FACE -> drawFace(c, cx, cy)
            KeyKind.ENTER -> drawReturn(c, cx, cy)
            KeyKind.SPACE -> {
                text.color = Colors.MUTED
                text.textSize = 13 * sp
                c.drawText(spaceLabel, cx, baseline(cy), text)
                if (auto) {
                    fill.color = Colors.ACCENT
                    c.drawCircle(cx + text.measureText(spaceLabel) / 2 + 6 * dp, cy - 3 * dp, 2.5f * dp, fill)
                }
                if (showArrows) {
                    text.color = Colors.DIM
                    text.textSize = 12 * sp
                    val half = text.measureText(spaceLabel) / 2 + 14 * dp
                    c.drawText("‹", cx - half, baseline(cy), text)
                    c.drawText("›", cx + half, baseline(cy), text)
                }
            }
            else -> {
                val label = when (k.kind) {
                    KeyKind.CHAR -> if (shift != Shift.OFF && k.alt.isNotEmpty()) k.alt else if (shift != Shift.OFF && k.label.length == 1) k.label.uppercase() else k.label
                    KeyKind.SHIFT -> if (shift == Shift.LOCK) "⇪" else "⇧"
                    else -> k.label
                }
                val big = k.kind == KeyKind.CHAR && label.length == 1 && label[0].isLetterOrDigit()
                text.textSize = (if (big) 22 else if (k.kind == KeyKind.CHAR) 19 else if (k.kind == KeyKind.BACKSPACE || k.kind == KeyKind.SHIFT) 17 else 15) * sp
                text.color = when (k.kind) {
                    KeyKind.CHAR -> Colors.FG
                    KeyKind.ENTER -> Colors.ENTER_FG
                    KeyKind.SHIFT -> if (shift == Shift.OFF) Colors.MUTED else Colors.ACCENT
                    else -> Colors.MUTED
                }
                c.drawText(label, cx, baseline(cy), text)
                if (k.hint.isNotEmpty()) {
                    text.textSize = 9 * sp
                    text.color = Colors.DIM
                    c.drawText(k.hint, p.face.right - 6 * dp, p.face.top + 11 * dp, text)
                }
            }
        }
    }

    private fun baseline(cy: Float) = cy - (text.descent() + text.ascent()) / 2

    /** ↵, drawn: a line down and back, with an arrowhead. */
    private fun drawReturn(c: Canvas, cx: Float, cy: Float) {
        stroke.color = Colors.ENTER_FG
        stroke.strokeWidth = 2 * dp
        val path = android.graphics.Path().apply {
            moveTo(cx + 7 * dp, cy - 7 * dp)
            lineTo(cx + 7 * dp, cy + 2 * dp)
            lineTo(cx - 7 * dp, cy + 2 * dp)
            moveTo(cx - 3 * dp, cy - 2.5f * dp)
            lineTo(cx - 7.5f * dp, cy + 2 * dp)
            lineTo(cx - 3 * dp, cy + 6.5f * dp)
        }
        c.drawPath(path, stroke)
        stroke.strokeWidth = 1.7f * dp
    }

    private fun drawFace(c: Canvas, cx: Float, cy: Float) {
        stroke.color = Colors.MUTED
        val rr = 8.5f * dp
        c.drawCircle(cx, cy, rr, stroke)
        fill.color = Colors.MUTED
        c.drawCircle(cx - 3 * dp, cy - 2.5f * dp, 1.1f * dp, fill)
        c.drawCircle(cx + 3 * dp, cy - 2.5f * dp, 1.1f * dp, fill)
        c.drawArc(RectF(cx - 4.5f * dp, cy - 2 * dp, cx + 4.5f * dp, cy + 5 * dp), 20f, 140f, false, stroke)
    }

    // --- touch ---

    private val handler = Handler(Looper.getMainLooper())
    private var pressed: Placed? = null
    private var pointerId = -1
    private var downAt = 0L
    private var downX = 0f
    private var downY = 0f
    private var swiped = 0f
    private var popup: Popup? = null
    private var held = false

    private val holdTask = Runnable {
        val p = pressed ?: return@Runnable
        when (p.key.kind) {
            KeyKind.BACKSPACE -> { repeats = 0; held = true; handler.postDelayed(repeatTask, WORD_HOLD_MS - 400) }
            KeyKind.SPACE -> if (languages.size > 1) openLanguages(p)
            KeyKind.SYMBOLS -> { held = true; listener.onHoldSymbols() }
            else -> if (p.key.popups.isNotEmpty()) openPopup(p)
        }
    }
    private var repeats = 0
    private val repeatTask: Runnable = object : Runnable {
        override fun run() {
            val p = pressed ?: return
            if (p.key.kind == KeyKind.BACKSPACE) {
                repeats += 1
                val held = android.os.SystemClock.uptimeMillis() - downAt
                listener.onRepeat(p.key, held)
                handler.postDelayed(this, wordGap(repeats))
            }
        }
    }

    /** Each key's text, kind and centre on the screen, as JSON (for `keys keymap`: tests tap any layout). */
    fun keymap(): String {
        val at = IntArray(2).also { getLocationOnScreen(it) }
        val a = org.json.JSONArray()
        placed.forEach { p ->
            a.put(org.json.JSONObject().put("kind", p.key.kind.name.lowercase()).put("text", p.key.text).put("alt", p.key.alt)
                .put("x", (at[0] + p.cell.centerX()).toInt()).put("y", (at[1] + p.cell.centerY()).toInt()))
        }
        return a.toString()
    }

    private fun keyAt(x: Float, y: Float): Placed? {
        placed.firstOrNull { it.cell.contains(x, y) }?.let { return it }
        // Between or beyond the keys: the nearest one.
        return placed.minByOrNull { p ->
            val dx = max(0f, max(p.cell.left - x, x - p.cell.right))
            val dy = max(0f, max(p.cell.top - y, y - p.cell.bottom))
            dx * dx + dy * dy
        }
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(e: MotionEvent): Boolean {
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> {
                // A second finger: the first one's key counts as typed now.
                if (pressed != null) release(commit = popup == null && !held)
                val i = e.actionIndex
                val p = keyAt(e.getX(i), e.getY(i)) ?: return true
                pressed = p
                pointerId = e.getPointerId(i)
                downX = e.getX(i)
                downY = e.getY(i)
                downAt = android.os.SystemClock.uptimeMillis()
                swiped = 0f
                held = false
                listener.onPress(p.key)
                if (p.key.kind == KeyKind.BACKSPACE) {
                    listener.onKey(p.key)
                    held = true
                    handler.postDelayed(holdTask, 400)
                } else {
                    handler.postDelayed(holdTask, 300)
                }
                invalidate()
            }
            MotionEvent.ACTION_MOVE -> {
                val i = e.findPointerIndex(pointerId)
                val p = pressed
                if (i >= 0 && p != null) {
                    val x = e.getX(i)
                    val y = e.getY(i)
                    if (swipe && popup == null && p.key.kind == KeyKind.CHAR && p.key.text.all { it.isLetter() }) {
                        if (!swiping && kotlin.math.hypot(x - downX, y - downY) > p.cell.width() * 0.7f) {
                            swiping = true
                            handler.removeCallbacks(holdTask)
                            swipePath = StringBuilder(p.key.text)
                            trail.reset()
                            trail.moveTo(downX, downY)
                        }
                        if (swiping) {
                            trail.lineTo(x, y)
                            val k = keyAt(x, y)?.key
                            if (k != null && k.kind == KeyKind.CHAR && k.text.length == 1 && k.text[0].isLetter() && swipePath.last() != k.text[0]) swipePath.append(k.text)
                            invalidate()
                            return true
                        }
                    }
                    val pop = popup
                    if (pop != null) {
                        val w = pop.box.width() / pop.cols
                        val rows = (pop.choices.size + pop.cols - 1) / pop.cols
                        val h = pop.box.height() / rows
                        val col = ((x - pop.box.left) / w).toInt().coerceIn(0, pop.cols - 1)
                        // Rows count up from the one nearest the key.
                        val row = ((pop.box.bottom - e.getY(i)) / h).toInt().coerceIn(0, rows - 1)
                        val sel = (row * pop.cols + col).coerceIn(0, pop.choices.size - 1)
                        if (sel != pop.selected) { popup = pop.copy(selected = sel); listener.onPopup(popup) }
                    } else if (p.key.kind == KeyKind.SPACE) {
                        val step = 48 * dp
                        val dx = x - downX - swiped
                        if (abs(dx) > step) {
                            handler.removeCallbacks(holdTask)
                            swiped += if (dx > 0) step else -step
                            held = true
                            listener.onSpaceSwipe(if (dx > 0) 1 else -1)
                        }
                    }
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> {
                if (e.getPointerId(e.actionIndex) == pointerId) release(commit = true)
            }
            MotionEvent.ACTION_CANCEL -> release(commit = false)
        }
        return true
    }

    private fun release(commit: Boolean) {
        handler.removeCallbacks(holdTask)
        handler.removeCallbacks(repeatTask)
        if (swiping) {
            swiping = false
            pressed = null
            pointerId = -1
            trail.reset()
            if (commit && swipePath.length >= 2) listener.onSwipe(swipePath.toString())
            invalidate()
            return
        }
        val p = pressed
        val pop = popup
        pressed = null
        popup = null
        if (pop != null) {
            listener.onPopup(null)
            if (commit) {
                if (pop.codes.isNotEmpty()) listener.onLanguage(pop.codes[pop.selected]) else listener.onChoice(pop.choices[pop.selected])
            }
        } else if (p != null && commit && !held) {
            listener.onKey(p.key)
        }
        held = false
        pointerId = -1
        invalidate()
    }

    private fun openLanguages(p: Placed) {
        val w = 46 * dp
        val cols = min(languages.size, 7)
        val rows = (languages.size + cols - 1) / cols
        val boxW = w * cols
        val left = max(side, min(p.face.centerX() - boxW / 2, width - side - boxW))
        val top = p.face.top - rows * rowH - 6 * dp
        val at = languages.indexOfFirst { it.first == language }.coerceAtLeast(0)
        popup = Popup(RectF(p.face), RectF(left, top, left + boxW, top + rows * rowH), languages.map { it.second }, at, languages.map { it.first }, cols)
        listener.onPopup(popup)
    }

    private fun openPopup(p: Placed) {
        val choices = p.key.popups.map { if (shift != Shift.OFF && it.length == 1) it.uppercase() else it }
        // Long lists (Vietnamese vowels) wrap into two rows at most, the
        // first nearest the key: more above the keys would leave the window.
        val cols = min(choices.size, max(8, (choices.size + 1) / 2))
        val w = min(min(p.face.width(), 40 * dp).coerceAtLeast(34 * dp), (width - 2 * side) / cols)
        val rows = (choices.size + cols - 1) / cols
        val boxW = w * cols
        val left = max(side, min(p.face.centerX() - w / 2, width - side - boxW))
        val top = p.face.top - rows * rowH - 6 * dp
        popup = Popup(RectF(p.face), RectF(left, top, left + boxW, top + rows * rowH), choices, 0, cols = cols)
        listener.onPopup(popup)
    }
}
