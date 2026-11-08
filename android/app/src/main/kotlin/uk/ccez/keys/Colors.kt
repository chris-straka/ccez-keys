package uk.ccez.keys

import androidx.compose.ui.graphics.Color

/** The keyboard's colours (dark only), as in docs/layout.html. */
object Colors {
    const val KB_BG = 0xFF121212.toInt()
    const val KEY = 0xFF2C2C2E.toInt()
    const val MOD = 0xFF1F1F21.toInt()
    const val KEY_PRESSED = 0xFF46464A.toInt()
    const val FG = 0xFFEDEDED.toInt()
    const val MUTED = 0xFF9A9A9A.toInt()
    const val DIM = 0xFF6C6C6C.toInt()
    const val ACCENT = 0xFFFF8A80.toInt()
    const val ENTER = 0xFF3D4C78.toInt()
    const val ENTER_FG = 0xFFDFE6FF.toInt()
    const val FACE_ON = 0xFF3A2A2A.toInt()

    val bg = Color(KB_BG)
    val key = Color(KEY)
    val mod = Color(MOD)
    val fg = Color(FG)
    val muted = Color(MUTED)
    val dim = Color(DIM)
    val accent = Color(ACCENT)
    val faceOn = Color(FACE_ON)
    val line = Color(0xFF2A2A2A)
    val chip = Color(0xFF26262A)
    val popup = Color(0xFF3A3A3E)
    val popupSel = Color(0xFF5A4644)
    val switchOn = Color(0xFF6B3A36)
    val switchOff = Color(0xFF3A3A3C)
}
