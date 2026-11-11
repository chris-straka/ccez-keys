package uk.ccez.keys.overlay

/**
 * Where words can go on screen, as on the Mac (desktop/src/idle.rs): cells
 * of a grid over the screen that no content touches. Pure (its own [Box],
 * not Android's Rect), so it can be tested; [ScreenWordsService] feeds it
 * node bounds and never text.
 */
object FreeSpace {
    /** A node this big next to its window is a backdrop, not content. */
    const val BIG = 0.08

    data class Box(val left: Int, val top: Int, val right: Int, val bottom: Int) {
        fun width() = right - left
        fun height() = bottom - top
        fun centerX() = (left + right) / 2
        fun centerY() = (top + bottom) / 2
    }

    private fun hits(cell: Box, r: Box) = r.left < cell.right && cell.left < r.right && r.top < cell.bottom && cell.top < r.bottom

    /** The cells of a `cols` × `rows` grid over `screen` that no blocker touches, spread over the screen. */
    fun freeCells(screen: Box, blockers: List<Box>, cols: Int, rows: Int): List<Box> {
        if (cols <= 0 || rows <= 0 || screen.width() <= 0 || screen.height() <= 0) return emptyList()
        val cw = screen.width() / cols
        val ch = screen.height() / rows
        val out = ArrayList<Box>()
        for (r in 0 until rows) {
            for (c in 0 until cols) {
                val cell = Box(screen.left + c * cw, screen.top + r * ch, screen.left + (c + 1) * cw, screen.top + (r + 1) * ch)
                if (blockers.none { hits(cell, it) }) out += cell
            }
        }
        // Every other cell first, so a few words don't bunch in one corner.
        return out.filterIndexed { i, _ -> i % 2 == 0 } + out.filterIndexed { i, _ -> i % 2 == 1 }
    }

    /** Classes that are content by kind: text, images and controls (their text is never read). */
    private val CONTENT = setOf(
        "android.widget.TextView",
        "android.widget.EditText",
        "android.widget.Button",
        "android.widget.ImageButton",
        "android.widget.ImageView",
        "android.widget.CheckBox",
        "android.widget.RadioButton",
        "android.widget.Switch",
        "android.widget.ToggleButton",
        "android.widget.SeekBar",
        "android.widget.Spinner",
        "android.widget.ProgressBar",
        "android.webkit.WebView",
        "android.view.SurfaceView",
        "android.view.TextureView",
        "android.widget.VideoView",
    )

    /** A node of this class holds something a word must keep clear of. */
    fun isContentClass(className: CharSequence?): Boolean {
        val c = className?.toString() ?: return false
        if (c in CONTENT) return true
        // App widgets and Compose draw their own kinds; the common suffixes still say what they are.
        return c.endsWith("TextView") || c.endsWith("EditText") || c.endsWith("Button") || c.endsWith("ImageView") ||
            c.endsWith("VideoView") || c.endsWith("SurfaceView") || c.endsWith("TextureView") || c.endsWith("WebView")
    }

    private val VIDEO = setOf("android.view.SurfaceView", "android.view.TextureView", "android.widget.VideoView")

    /** A video's picture, by class (players draw into one of these). */
    fun isVideoClass(className: CharSequence?): Boolean {
        val c = className?.toString() ?: return false
        return c in VIDEO || c.endsWith("VideoView") || c.endsWith("PlayerView")
    }

    /**
     * The screenshot UI is up (its window appeared): SystemUI's screenshot
     * panel, by package, window title or a view id. Words hide then, so
     * they never land in a screenshot.
     */
    fun isScreenshotWindow(pkg: CharSequence?, title: CharSequence?, className: CharSequence?, viewIds: List<String>): Boolean {
        val p = pkg?.toString().orEmpty()
        if (p.contains("screenshot", ignoreCase = true)) return true
        if (title?.toString()?.contains("screenshot", ignoreCase = true) == true) return true
        if (className?.toString()?.contains("Screenshot") == true) return true
        if (p == "com.android.systemui" && viewIds.any { it.contains("screenshot", ignoreCase = true) }) return true
        return false
    }
}
