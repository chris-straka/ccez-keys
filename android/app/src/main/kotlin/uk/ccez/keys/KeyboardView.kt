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

/** A held key's choices, in this view's coordinates. */
data class Popup(val key: RectF, val box: RectF, val choices: List<String>, val selected: Int)

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
    }

    var layout: Layout? = null
        set(v) { field = v; requestLayout(); placeKeys(); invalidate() }
    var shift = Shift.OFF
        set(v) { if (field != v) { field = v; invalidate() } }
    var spaceLabel = ""
        set(v) { if (field != v) { field = v; invalidate() } }
    var showArrows = false
        set(v) { if (field != v) { field = v; invalidate() } }
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
        val unit = (width - 2 * side) / 10f
        val out = ArrayList<Placed>()
        l.rows.forEachIndexed { r, row ->
            val units = row.keys.sumOf { it.width.toDouble() }.toFloat()
            val onlyChars = row.keys.all { it.kind == KeyKind.CHAR }
            // A short row of letters sits centred; any row with other keys spans the width.
            val u = if (onlyChars) unit else (width - 2 * side) / units
            var x = side + if (onlyChars) (10 - units) * unit / 2 else 0f
            val top = padTop + r * (rowH + rowGap)
            row.keys.forEach { k ->
                val w = k.width * u
                val cell = RectF(x, top - rowGap / 2, x + w, top + rowH + rowGap / 2)
                val face = RectF(x + gap / 2, top, x + w - gap / 2, top + rowH)
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
                    KeyKind.CHAR -> if (shift != Shift.OFF && k.label.length == 1) k.label.uppercase() else k.label
                    KeyKind.SHIFT -> if (shift == Shift.LOCK) "⇪" else "⇧"
                    else -> k.label
                }
                val big = k.kind == KeyKind.CHAR && label.length == 1 && label[0].isLetterOrDigit()
                text.textSize = (if (big) 22 else if (k.kind == KeyKind.CHAR) 19 else if (k.kind == KeyKind.BACKSPACE || k.kind == KeyKind.SHIFT) 21 else 15) * sp
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
    private var downX = 0f
    private var swiped = 0f
    private var popup: Popup? = null
    private var held = false

    private val holdTask = Runnable {
        val p = pressed ?: return@Runnable
        when (p.key.kind) {
            KeyKind.BACKSPACE -> { listener.onKey(p.key); held = true; handler.postDelayed(repeatTask, 50) }
            else -> if (p.key.popups.isNotEmpty()) openPopup(p)
        }
    }
    private val repeatTask: Runnable = object : Runnable {
        override fun run() {
            val p = pressed ?: return
            if (p.key.kind == KeyKind.BACKSPACE) {
                listener.onKey(p.key)
                handler.postDelayed(this, 50)
            }
        }
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
                    val pop = popup
                    if (pop != null) {
                        val w = pop.box.width() / pop.choices.size
                        val sel = ((x - pop.box.left) / w).toInt().coerceIn(0, pop.choices.size - 1)
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
        val p = pressed
        val pop = popup
        pressed = null
        popup = null
        if (pop != null) {
            listener.onPopup(null)
            if (commit) listener.onChoice(pop.choices[pop.selected])
        } else if (p != null && commit && !held) {
            listener.onKey(p.key)
        }
        held = false
        pointerId = -1
        invalidate()
    }

    private fun openPopup(p: Placed) {
        val choices = p.key.popups.map { if (shift != Shift.OFF && it.length == 1) it.uppercase() else it }
        val w = min(p.face.width(), 40 * dp).coerceAtLeast(34 * dp)
        val boxW = w * choices.size
        val left = max(side, min(p.face.centerX() - w / 2, width - side - boxW))
        val top = p.face.top - rowH - 6 * dp
        popup = Popup(RectF(p.face), RectF(left, top, left + boxW, top + rowH), choices, 0)
        listener.onPopup(popup)
    }
}
