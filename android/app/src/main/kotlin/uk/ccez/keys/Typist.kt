package uk.ccez.keys

import android.media.AudioManager
import android.os.Build
import android.os.SystemClock
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.text.InputType
import android.view.HapticFeedbackConstants
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import java.text.BreakIterator
import kotlinx.coroutines.launch
import uk.ccez.keys.core.Key
import uk.ccez.keys.core.KeyKind
import uk.ccez.keys.core.Layout
import uk.ccez.keys.core.Page

/** What the bar above the keys shows. */
sealed interface Bar {
    data object Buttons : Bar
    /** Three cells; `typed` marks the cell that keeps the word as typed. */
    data class Words(val left: String?, val mid: String?, val right: String?, val midIsFix: Boolean, val typed: String?) : Bar
}

/**
 * Turns keys into text: the word being typed stays "composing" (underlined
 * in the app) until a space or a mark ends it, and then it's corrected if
 * the core says so. Backspace right after a correction puts the typed word
 * back and trusts it from then on.
 */
class Typist(private val service: KeysService, private val ui: UiState) : KeyboardView.Listener {
    private val app = App.instance
    private val engine get() = app.engine
    val view: KeyboardView = KeyboardView(service, this)
    val strip = uk.ccez.keys.learn.Strip(ui)
    val tips = uk.ccez.keys.learn.Tips(ui)
    val ask = uk.ccez.keys.learn.Ask(ui)

    private var lang = engine.currentLanguage()
    private var page = Page.LETTERS
    private val word = StringBuilder()
    private var shift = Shift.OFF
    private var lastShiftTap = 0L
    private var lastSpaceAt = 0L
    private var fix: Pair<String, String>? = null // typed → what it became
    private var autoSpace = false
    private var editor: EditorInfo? = null
    private var password = false
    private var noLearn = false
    private var noCorrect = false

    private val ic: InputConnection? get() = service.currentInputConnection

    fun start(info: EditorInfo) {
        editor = info
        val cls = info.inputType and InputType.TYPE_MASK_CLASS
        val variation = info.inputType and InputType.TYPE_MASK_VARIATION
        password = (cls == InputType.TYPE_CLASS_TEXT && variation in PASSWORDS) ||
            (cls == InputType.TYPE_CLASS_NUMBER && variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD)
        noLearn = password || info.imeOptions and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING != 0
        noCorrect = password || variation in ADDRESSES || info.inputType and InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS != 0
        page = if (cls == InputType.TYPE_CLASS_NUMBER || cls == InputType.TYPE_CLASS_PHONE || cls == InputType.TYPE_CLASS_DATETIME) Page.SYMBOLS else Page.LETTERS
        word.clear()
        fix = null
        autoSpace = false
        recent.clear()
        lang = engine.currentLanguage().also { l -> app.scope.launch { app.ensureWords(l) } }
        ui.panel = Panel.KEYS
        ui.password = password
        ui.chatApp = info.packageName in engine.settingStrings("apps.chat")
        refresh()
        updateShift()
        updateBar()
    }

    fun finish() {
        word.clear()
        engine.save()
    }

    /** Layout, size and labels again (after a setting or language change). */
    fun refresh() {
        val face = engine.settingBool("buttons.face")
        var layout = withoutFace(engine.layout(lang, page), !face)
        if (page == Page.LETTERS && engine.settingBool("typing.numbers")) layout = withNumbers(layout)
        view.layout = layout
        view.scale = (engine.setting("keyboard.height").toIntOrNull() ?: 100) / 100f
        val langs = engine.typingLanguages()
        view.spaceLabel = uk.ccez.keys.ui.Langs.of(service, lang)?.own ?: lang
        view.showArrows = langs.size > 1
        view.languages = langs.map { it to (uk.ccez.keys.ui.Langs.of(service, it)?.flag ?: it) }
        view.language = lang
        ui.keysHeightDp = keysHeightDp(view.scale, layout.rows.size)
    }

    // --- keys ---

    override fun onPress(key: Key) {
        buzz(HapticFeedbackConstants.KEYBOARD_TAP)
        if (engine.settingBool("typing.sound")) {
            val am = service.getSystemService(AudioManager::class.java)
            am?.playSoundEffect(
                when (key.kind) {
                    KeyKind.BACKSPACE -> AudioManager.FX_KEYPRESS_DELETE
                    KeyKind.SPACE -> AudioManager.FX_KEYPRESS_SPACEBAR
                    KeyKind.ENTER -> AudioManager.FX_KEYPRESS_RETURN
                    else -> AudioManager.FX_KEYPRESS_STANDARD
                },
            )
        }
    }

    override fun onPopup(popup: Popup?) {
        val before = ui.popup
        ui.popup = popup
        when {
            popup != null && before == null -> buzz(HapticFeedbackConstants.LONG_PRESS)
            popup != null && before != null && popup.selected != before.selected -> buzz(HapticFeedbackConstants.CLOCK_TICK)
        }
    }

    private val vibrator: Vibrator? =
        if (Build.VERSION.SDK_INT >= 31) service.getSystemService(VibratorManager::class.java)?.defaultVibrator
        else @Suppress("DEPRECATION") service.getSystemService(Vibrator::class.java)

    /**
     * Through the vibrator itself: View.performHapticFeedback is skipped
     * when the phone's touch feedback is off, and the owner felt nothing.
     */
    private fun buzz(kind: Int) {
        if (!engine.settingBool("typing.vibrate")) return
        val v = vibrator ?: return
        if (!v.hasVibrator()) return
        val (primitive, scale, fallback) = when (kind) {
            HapticFeedbackConstants.KEYBOARD_TAP -> Triple(VibrationEffect.Composition.PRIMITIVE_TICK, 0.7f, VibrationEffect.EFFECT_TICK)
            HapticFeedbackConstants.CLOCK_TICK -> Triple(VibrationEffect.Composition.PRIMITIVE_TICK, 0.4f, VibrationEffect.EFFECT_TICK)
            else -> Triple(VibrationEffect.Composition.PRIMITIVE_CLICK, 0.8f, VibrationEffect.EFFECT_CLICK)
        }
        val effect = if (Build.VERSION.SDK_INT >= 30 && v.areAllPrimitivesSupported(primitive)) {
            VibrationEffect.startComposition().addPrimitive(primitive, scale).compose()
        } else {
            VibrationEffect.createPredefined(fallback)
        }
        runCatching { v.vibrate(effect) }
    }

    override fun onChoice(text: String) = type(text)

    override fun onRepeat(key: Key, count: Int) {
        if (count > WORDS_AFTER) deleteWord() else backspace()
        if (count > 1) buzz(HapticFeedbackConstants.CLOCK_TICK)
    }

    /** Back to the start of the word (or the run of marks) before the cursor. */
    private fun deleteWord() {
        val ic = ic ?: return
        fix = null
        autoSpace = false
        if (word.isNotEmpty()) {
            word.clear()
            ic.setComposingText("", 1)
        } else {
            val before = ic.getTextBeforeCursor(64, 0)?.toString().orEmpty()
            val trimmed = before.trimEnd()
            val last = trimmed.lastOrNull()
            val body = when {
                last == null -> ""
                isWordChar(last) -> trimmed.takeLastWhile(::isWordChar)
                else -> trimmed.takeLastWhile { !isWordChar(it) && !it.isWhitespace() }
            }
            val n = before.length - trimmed.length + body.length
            if (n > 0) ic.deleteSurroundingText(n, 0)
        }
        updateShift()
        updateBar()
    }

    override fun onKey(key: Key) {
        if (key.kind != KeyKind.BACKSPACE) fix = null
        when (key.kind) {
            KeyKind.CHAR -> type(key.text)
            KeyKind.SHIFT -> {
                val now = SystemClock.uptimeMillis()
                shift = when {
                    now - lastShiftTap < 300 -> Shift.LOCK
                    shift == Shift.OFF -> Shift.ONCE
                    else -> Shift.OFF
                }
                lastShiftTap = now
                view.shift = shift
            }
            KeyKind.BACKSPACE -> backspace()
            KeyKind.SYMBOLS -> setPage(Page.SYMBOLS)
            KeyKind.MORE -> setPage(Page.SYMBOLS_MORE)
            KeyKind.LETTERS -> setPage(Page.LETTERS)
            KeyKind.FACE -> {
                endWord(correct = false)
                ui.panel = Panel.FACE
            }
            KeyKind.SPACE -> space()
            KeyKind.ENTER -> enter()
        }
    }

    override fun onLanguage(code: String) {
        view.auto = false
        if (code == lang) return
        buzz(HapticFeedbackConstants.CONTEXT_CLICK)
        endWord(correct = false)
        engine.setCurrentLanguage(code)
        lang = code
        app.scope.launch { app.ensureWords(lang) }
        refresh()
        updateBar()
    }

    override fun onSpaceSwipe(step: Int) {
        view.auto = false
        buzz(HapticFeedbackConstants.CONTEXT_CLICK)
        endWord(correct = false)
        lang = engine.stepLanguage(step)
        app.scope.launch { app.ensureWords(lang) }
        refresh()
        updateBar()
    }

    private fun setPage(p: Page) {
        page = p
        refresh()
    }

    fun type(text: String) {
        ui.ask?.let { a ->
            val t = if (shift != Shift.OFF) text.uppercase() else text
            ui.ask = a.copy(input = a.input + t)
            if (shift == Shift.ONCE) setShift(Shift.OFF)
            return
        }
        val ic = ic ?: return
        if (autoSpaceSwap(text, ic)) return
        if (text.length <= 2 && text.all(::isWordChar) && !password) {
            val t = if (shift != Shift.OFF) text.uppercase() else text
            word.append(t)
            ic.setComposingText(word, 1)
            if (shift == Shift.ONCE) setShift(Shift.OFF)
            updateBar()
            return
        }
        endWord(correct = true)
        ic.commitText(if (shift != Shift.OFF && text.length == 1) text.uppercase() else text, 1)
        if (shift == Shift.ONCE) setShift(Shift.OFF)
        if (text in ".!?") updateShift()
        updateBar()
    }

    /** After a word we spaced ourselves, ".,!?" go before that space. */
    private fun autoSpaceSwap(text: String, ic: InputConnection): Boolean {
        if (!autoSpace || word.isNotEmpty()) { autoSpace = false; return false }
        autoSpace = false
        if (text.length == 1 && text in ".,!?;:)") {
            val french = lang == "fr" && text in "!?;:"
            if (ic.getTextBeforeCursor(1, 0) == " ") {
                ic.deleteSurroundingText(1, 0)
                ic.commitText((if (french) " " else "") + text + " ", 1)
                if (text in ".!?") updateShift()
                autoSpace = true
                updateBar()
                return true
            }
        }
        return false
    }

    /** Commits the composing word, corrected when `correct`. Returns it. */
    private fun endWord(correct: Boolean): String? {
        if (word.isEmpty()) return null
        val ic = ic ?: return null
        val typed = word.toString()
        val prev = prevWord(typed.length)
        val fixed = if (correct && !noCorrect) engine.suggest(lang, typed).correction else null
        val out = fixed ?: typed
        ic.commitText(out, 1)
        fix = fixed?.let { typed to it }
        word.clear()
        if (!noLearn) engine.learn(lang, prev, out, false)
        noticeLanguage(out)
        return out
    }

    /** The last words typed in this field, for noticing their language. */
    private val recent = ArrayDeque<String>()

    private fun noticeLanguage(w: String) {
        if (password || !w.any { it.isLetter() }) return
        recent.addLast(w)
        while (recent.size > 2) recent.removeFirst()
        val to = engine.detectLanguage(lang, recent.toList()) ?: return
        lang = to
        engine.setCurrentLanguage(to)
        app.scope.launch { app.ensureWords(to) }
        recent.clear()
        view.auto = true
        refresh()
        app.log("info", "language-noticed", "to" to to)
    }

    private fun space() {
        ui.ask?.let { a -> ui.ask = a.copy(input = a.input + " "); return }
        val ic = ic ?: return
        val now = SystemClock.uptimeMillis()
        if (word.isNotEmpty()) {
            endWord(correct = true)
            ic.commitText(" ", 1)
        } else {
            val before = ic.getTextBeforeCursor(2, 0)?.toString().orEmpty()
            if (engine.settingBool("typing.double_space_period") && now - lastSpaceAt < 1000 &&
                before.length == 2 && before[1] == ' ' && before[0].isLetterOrDigit()
            ) {
                ic.deleteSurroundingText(1, 0)
                ic.commitText(". ", 1)
            } else {
                ic.commitText(" ", 1)
            }
        }
        lastSpaceAt = now
        autoSpace = false
        updateShift()
        updateBar()
    }

    private fun backspace() {
        ui.ask?.let { a -> if (a.input.isNotEmpty()) ui.ask = a.copy(input = a.input.substring(0, a.input.offsetByCodePoints(a.input.length, -1))); return }
        val ic = ic ?: return
        autoSpace = false
        val f = fix
        fix = null
        if (f != null && word.isEmpty()) {
            val (typed, fixed) = f
            val tail = fixed + " "
            if (ic.getTextBeforeCursor(tail.length, 0)?.toString() == tail) {
                ic.deleteSurroundingText(tail.length, 0)
                word.append(typed)
                ic.setComposingText(word, 1)
                if (!noLearn) engine.learn(lang, prevWord(typed.length), typed, true)
                buzz(HapticFeedbackConstants.CONTEXT_CLICK)
                updateBar()
                return
            }
        }
        if (word.isNotEmpty()) {
            word.setLength(word.offsetByCodePoints(word.length, -1))
            ic.setComposingText(word, 1)
            updateBar()
            if (word.isEmpty()) updateShift()
            return
        }
        if (!ic.getSelectedText(0).isNullOrEmpty()) {
            ic.commitText("", 1)
        } else {
            val before = ic.getTextBeforeCursor(16, 0)?.toString().orEmpty()
            if (before.isEmpty()) {
                service.sendDownUpKeyEvents(android.view.KeyEvent.KEYCODE_DEL)
            } else {
                // One whole character, emoji and flags included.
                val it = BreakIterator.getCharacterInstance()
                it.setText(before)
                val start = it.preceding(before.length).coerceAtLeast(0)
                ic.deleteSurroundingText(before.length - start, 0)
            }
        }
        updateShift()
        updateBar()
    }

    private fun enter() {
        if (ui.ask != null) { ask.send(); return }
        val ic = ic ?: return
        endWord(correct = false)
        val info = editor
        val action = (info?.imeOptions ?: 0) and EditorInfo.IME_MASK_ACTION
        val noAction = (info?.imeOptions ?: 0) and EditorInfo.IME_FLAG_NO_ENTER_ACTION != 0
        val multiLine = (info?.inputType ?: 0) and InputType.TYPE_TEXT_FLAG_MULTI_LINE != 0
        if (!noAction && !multiLine && action != EditorInfo.IME_ACTION_NONE && action != EditorInfo.IME_ACTION_UNSPECIFIED) {
            ic.performEditorAction(action)
        } else {
            service.sendKeyChar('\n')
        }
        updateShift()
        updateBar()
    }

    // --- the bar ---

    /** A cell tapped: that word, then a space. */
    fun pick(text: String, isTyped: Boolean) {
        val ic = ic ?: return
        val prev = prevWord(word.length)
        ic.commitText("$text ", 1)
        word.clear()
        fix = null
        if (!noLearn) engine.learn(lang, prev, text, isTyped)
        autoSpace = true
        updateShift()
        updateBar()
    }

    fun paste(text: String) {
        endWord(correct = false)
        ic?.commitText(text, 1)
        updateShift()
        updateBar()
    }

    fun insertFace(text: String) {
        endWord(correct = false)
        ic?.commitText(text, 1)
        updateBar()
    }

    /** For the face panel's own bottom row. */
    fun press(kind: KeyKind) {
        when (kind) {
            KeyKind.SPACE -> space()
            KeyKind.BACKSPACE -> backspace()
            KeyKind.ENTER -> enter()
            else -> {}
        }
    }

    fun showKeys() {
        ui.panel = Panel.KEYS
        refresh()
    }

    private fun updateBar() {
        ui.card = null
        val sentence = if (password) "" else sentence()
        strip.update(sentence, lang, allowed = !password)
        tips.update(sentence, lang, allowed = !password, ended = sentence.trimEnd().lastOrNull()?.let { it in ".!?。！？" } == true)
        if (password) { ui.bar = Bar.Buttons; ui.glosses = emptyList(); return }
        updateWords()
        strip.gloss((ui.bar as? Bar.Words)?.mid?.takeIf { word.isNotEmpty() }, lang)
    }

    private fun updateWords() {
        if (word.isNotEmpty()) {
            val typed = word.toString()
            val s = engine.suggest(lang, typed)
            val mid = s.correction ?: if (s.typedKnown) typed else s.words.firstOrNull() ?: typed
            val rest = s.words.filter { !it.equals(mid, ignoreCase = true) }
            val left = if (s.correction != null || !s.typedKnown && mid != typed) typed else rest.getOrNull(0)
            val right = rest.firstOrNull { !it.equals(left, ignoreCase = true) && !it.equals(typed, ignoreCase = true) }
            ui.bar = Bar.Words(left, mid, right, s.correction != null, typed)
            return
        }
        val prev = prevWord(0)
        val next = if (prev.isNotEmpty() && !noLearn) engine.predict(lang, prev) else emptyList()
        ui.bar = if (next.isEmpty()) Bar.Buttons else Bar.Words(next.getOrNull(1), next[0], next.getOrNull(2), false, null)
    }

    // --- capitals and the selection ---

    private fun setShift(s: Shift) {
        shift = s
        view.shift = s
    }

    private fun updateShift() {
        if (shift == Shift.LOCK) return
        val info = editor ?: return
        val auto = engine.settingBool("typing.capitals") && word.isEmpty() && !password &&
            (ic?.getCursorCapsMode(info.inputType) ?: 0) != 0
        setShift(if (auto) Shift.ONCE else Shift.OFF)
    }

    fun onSelection(newStart: Int, newEnd: Int, candStart: Int, candEnd: Int) {
        if (word.isNotEmpty() && (candEnd == -1 || newStart != candEnd || newEnd != candEnd)) {
            // The cursor moved away from the word: keep it as typed.
            ic?.finishComposingText()
            word.clear()
            fix = null
            updateShift()
            updateBar()
        } else if (word.isEmpty() && candStart == -1) {
            updateShift()
            updateBar()
        }
    }

    /** The sentence being typed (the last one, when the cursor sits just after its end). */
    private fun sentence(): String {
        val before = ic?.getTextBeforeCursor(300, 0)?.toString().orEmpty()
        val t = before.trimEnd()
        val ends = ".!?。！？\n"
        val body = if (t.isNotEmpty() && t.last() in ends) t.dropLast(1) else t
        val start = body.indexOfLast { it in ends } + 1
        return t.substring(start).trim()
    }

    /** The sentence before the cursor swapped for `text` (held strip flag). */
    fun replaceSentence(text: String) {
        val ic = ic ?: return
        val s = sentence()
        if (s.isEmpty()) return
        val before = ic.getTextBeforeCursor(300, 0)?.toString().orEmpty()
        val at = before.lastIndexOf(s)
        if (at < 0) return
        word.clear()
        ic.finishComposingText()
        ic.deleteSurroundingText(before.length - at, 0)
        ic.commitText(text + before.substring(at + s.length).takeWhile { it == ' ' }, 1)
        buzz(HapticFeedbackConstants.CONTEXT_CLICK)
        updateShift()
        updateBar()
    }

    /** The tip's fix put in place of the sentence. */
    fun applyTip() {
        val t = ui.tip ?: return
        ui.tip = null
        replaceSentence(t.fix.text)
    }

    fun openAsk(about: String) {
        ui.card = null
        endWord(correct = false)
        ask.open(about)
    }

    fun closeAsk() {
        ask.close()
        updateShift()
    }

    fun markLearning(lang: String, word: String) {
        buzz(HapticFeedbackConstants.CONTEXT_CLICK)
        setStatus(lang, word, uk.ccez.keys.core.WordStatus.LEARNING)
    }

    fun setStatus(lang: String, word: String, status: uk.ccez.keys.core.WordStatus) {
        if (noLearn) return
        strip.setStatus(lang, word, status)
    }

    fun engineStatus(lang: String, word: String) = engine.wordStatus(lang, word)

    /** The glosses under a held suggestion, each to Learning in its language. */
    fun learnGlosses(glosses: List<uk.ccez.keys.learn.Gloss>) {
        if (noLearn) return
        buzz(HapticFeedbackConstants.CONTEXT_CLICK)
        glosses.forEach { engine.setWordStatus(it.lang, it.word, uk.ccez.keys.core.WordStatus.LEARNING) }
        uk.ccez.keys.learn.WordSync.soon()
    }

    /** The word before the cursor, skipping `skip` characters of the word being typed. */
    private fun prevWord(skip: Int): String {
        val before = ic?.getTextBeforeCursor(64 + skip, 0)?.toString().orEmpty()
        val t = before.dropLast(skip).trimEnd()
        return t.takeLastWhile(::isWordChar)
    }

    companion object {
        private val PASSWORDS = setOf(
            InputType.TYPE_TEXT_VARIATION_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD,
            InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD,
        )
        private val ADDRESSES = setOf(
            InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS,
            InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS,
            InputType.TYPE_TEXT_VARIATION_URI,
        )

        fun isWordChar(c: Char) = c.isLetterOrDigit() || c == '\'' || c == '’' || c == '-'

        fun keysHeightDp(scale: Float, rows: Int = 4) = 6 + 6 + rows * 44 * scale + (rows - 1) * 8

        /** SwiftKey's row of digits over the letters (Settings: Numbers). */
        private fun withNumbers(l: Layout): Layout {
            val digits = "1234567890".map { d -> Key(KeyKind.CHAR, d.toString(), d.toString(), "", emptyList(), 1f) }
            val top = l.rows.first().let { r -> r.copy(keys = r.keys.map { it.copy(hint = "", popups = it.popups.drop(1)) }) }
            return Layout(listOf(uk.ccez.keys.core.Row(digits), top) + l.rows.drop(1))
        }

        private fun withoutFace(l: Layout, drop: Boolean): Layout {
            if (!drop) return l
            return Layout(
                l.rows.map { r ->
                    val face = r.keys.firstOrNull { it.kind == KeyKind.FACE } ?: return@map r
                    r.copy(keys = r.keys.filter { it !== face }.map { if (it.kind == KeyKind.SPACE) it.copy(width = it.width + face.width) else it })
                },
            )
        }
    }
}
