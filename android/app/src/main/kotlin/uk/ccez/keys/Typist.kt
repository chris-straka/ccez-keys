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
    /** Japanese/Chinese: words for what's typed, the first the likeliest. */
    data class Convert(val words: List<uk.ccez.keys.core.Conversion>) : Bar
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
    val voice = uk.ccez.keys.learn.Voice(
        service,
        onState = { v -> ui.voice = v; ui.panel = if (v != null) Panel.VOICE else if (ui.panel == Panel.VOICE) Panel.KEYS else ui.panel },
        onText = { t -> dictated(t) },
    )

    /** Dictated text, spaced and capitalised like typed text. */
    private fun dictated(text: String) {
        val ic = ic ?: return
        acted()
        endWord(correct = false)
        val before = ic.getTextBeforeCursor(1, 0)?.toString().orEmpty()
        var t = text.trim()
        if (shift != Shift.OFF && t.isNotEmpty()) t = t[0].uppercase() + t.substring(1)
        ic.commitText((if (before.isNotEmpty() && !before[0].isWhitespace()) " " else "") + t + " ", 1)
        updateShift()
        updateBar()
    }

    private var lang = engine.currentLanguage()
    private var page = Page.LETTERS
    private val word = StringBuilder()
    /** Japanese: kana so far and romaji waiting for a vowel. Chinese: pinyin letters (in `conv`). */
    private val conv = StringBuilder()
    private var romaji = ""
    private fun converting() = lang == "ja" || lang == "zh"

    /** What the converter is given: kana (a last "n" as ん) or pinyin. */
    private fun convInput(): String =
        if (lang == "ja") conv.toString() + engine.romaji(romaji, true).kana else conv.toString()

    private fun showConv() {
        val ic = ic ?: return
        val shown = conv.toString() + romaji
        if (shown.isEmpty()) {
            ic.setComposingText("", 1)
            ui.bar = Bar.Buttons
            return
        }
        ic.setComposingText(shown, 1)
        // No converter yet (or an old one): fetch and build it; kana meanwhile.
        if (!engine.hasConverter(lang)) app.scope.launch { if (uk.ccez.keys.learn.Dictionaries.ensure(lang)) kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) { if (conv.isNotEmpty()) showConv() } }
        val words = engine.convert(lang, convInput())
        ui.bar = if (words.isEmpty()) Bar.Buttons else Bar.Convert(words)
    }

    /** A candidate picked: it goes in, the rest of the input stays to convert. */
    fun pickConversion(c: uk.ccez.keys.core.Conversion) {
        val ic = ic ?: return
        acted()
        val waiting = romaji
        val input = convInput()
        var rest = input.drop(c.consumed.toInt())
        ic.commitText(c.text, 1)
        // Letters still waiting for a vowel go back to waiting (a last "n" too, shown as ん).
        var back = ""
        if (lang == "ja" && waiting.isNotEmpty()) {
            val shown = engine.romaji(waiting, true).kana
            if (shown.isNotEmpty() && rest.endsWith(shown)) { rest = rest.dropLast(shown.length); back = waiting }
        }
        conv.setLength(0)
        conv.append(rest)
        romaji = back
        if (rest.isEmpty() && back.isEmpty()) {
            ui.bar = Bar.Buttons
            updateBar()
        } else {
            showConv()
        }
    }

    /** Leaves the input as typed (kana or pinyin letters). */
    private fun commitConv() {
        val ic = ic ?: return
        val shown = if (lang == "ja") convInput() else conv.toString()
        if (shown.isNotEmpty()) ic.commitText(shown, 1)
        conv.setLength(0)
        romaji = ""
    }

    /** Korean: the letters typed, which `word` shows joined into syllables. */
    private val jamo = StringBuilder()
    /** What `word` held before the jamo (a digit, a restored word): kept in front of them. */
    private var jamoBase = ""

    /** Vietnamese with Telex: the letters typed, which `word` shows as Telex writes them. */
    private val telexRaw = StringBuilder()
    private fun telexOn() = lang == "vi" && app.flag("typing.telex")

    private fun clearWord() {
        word.setLength(0)
        jamo.setLength(0)
        jamoBase = ""
        telexRaw.setLength(0)
        conv.setLength(0)
        romaji = ""
    }
    private var shift = Shift.OFF
    private var lastShiftTap = 0L
    private var lastSpaceAt = 0L
    private var fix: Pair<String, String>? = null // typed → what it became
    private var autoSpace = false
    private var editor: EditorInfo? = null
    private var password = false
    private var noLearn = false
    private var noCorrect = false
    /** Number, phone and date fields: digits go straight in, never composed. */
    private var numeric = false
    /** Fields with no text model (terminals): keys go as key events. */
    private var rawKeys = false

    /** When the typist last did something here (a key, a pick): selection news soon after is ours. */
    private var actedAt = 0L
    private fun acted() {
        actedAt = SystemClock.uptimeMillis()
        // Typing: the word to review gives way to the translation strip.
        if (ui.review != null) ui.review = null
        reviewSeq++
    }

    /** The field's connection, its text before the cursor remembered between edits (CachedInput). */
    private var cached: CachedInput? = null
    private val ic: InputConnection? get() {
        val raw = service.currentInputConnection ?: return null
        if (raw is CachedInput) return raw
        return cached?.takeIf { it.base === raw } ?: CachedInput(raw).also { it.base = raw; cached = it }
    }

    fun start(info: EditorInfo) {
        editor = info
        val cls = info.inputType and InputType.TYPE_MASK_CLASS
        val variation = info.inputType and InputType.TYPE_MASK_VARIATION
        password = (cls == InputType.TYPE_CLASS_TEXT && variation in PASSWORDS) ||
            (cls == InputType.TYPE_CLASS_NUMBER && variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD)
        noLearn = password || info.imeOptions and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING != 0
        noCorrect = password || variation in ADDRESSES || info.inputType and InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS != 0
        numeric = cls == InputType.TYPE_CLASS_NUMBER || cls == InputType.TYPE_CLASS_PHONE || cls == InputType.TYPE_CLASS_DATETIME
        rawKeys = info.inputType == InputType.TYPE_NULL
        swiped = null
        page = if (cls == InputType.TYPE_CLASS_NUMBER || cls == InputType.TYPE_CLASS_PHONE || cls == InputType.TYPE_CLASS_DATETIME) Page.SYMBOLS else Page.LETTERS
        clearWord()
        fix = null
        autoSpace = false
        recent.clear()
        // The language last typed in this app, else the current one.
        val remembered = appLang(info.packageName)?.takeIf { it in engine.typingLanguages() }
        if (remembered != null && remembered != engine.currentLanguage()) engine.setCurrentLanguage(remembered)
        lang = engine.currentLanguage().also { l -> app.scope.launch { app.ensureWords(l) } }
        // Japanese words, to notice Japanese typed in romaji on another keyboard.
        if ("ja" in engine.typingLanguages()) app.scope.launch { app.ensureWords("ja") }
        romajiSwitch = null
        ui.panel = Panel.KEYS
        ui.password = password
        ui.chatApp = info.packageName in app.strings("apps.chat")
        ui.polite = info.packageName in app.strings("apps.polite")
        refresh()
        updateShift()
        updateBar()
        ui.review = null
        reviewSeq++
        if (!noLearn && !numeric && !rawKeys && app.flag("learning.strip") && fieldEmpty()) review(null)
    }

    /** Nothing in the field, before or after the cursor. */
    private fun fieldEmpty(): Boolean {
        val c = ic ?: return false
        return c.getTextBeforeCursor(1, 0).isNullOrEmpty() && c.getTextAfterCursor(1, 0).isNullOrEmpty()
    }

    /** Bumped by any key: a word to review that's still loading doesn't show after one. */
    private var reviewSeq = 0

    /** The next word to review into the strip (not `shown`); nothing when none is due. */
    private fun review(shown: Pair<String, String>?) {
        val seq = reviewSeq
        app.scope.launch {
            val pick = runCatching { uk.ccez.keys.learn.Review.next(service, shown) }.getOrNull()
            val face = pick?.let { (l, w) -> runCatching { uk.ccez.keys.learn.Review.face(service, l, w) }.getOrNull() }
            kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
                if (seq == reviewSeq) {
                    ui.review = face
                    face?.let { uk.ccez.keys.learn.Review.seen(service, it.lang, it.word) }
                }
            }
        }
    }

    /** The word to review: its card. */
    fun reviewOpen() = ui.review?.let { strip.openWord(it.lang, it.word) }
    /** Known (everywhere), then the next word. */
    fun reviewKnown() = ui.review?.let { f -> ui.review = null; uk.ccez.keys.learn.Review.known(service, f.lang, f.word); buzz(HapticFeedbackConstants.CONFIRM); review(f.lang to f.word) }
    /** Later (1, 3, 7, 14, 30 days), then the next word. (The word goes at once: a second tap can't count twice.) */
    fun reviewLater() = ui.review?.let { f -> ui.review = null; uk.ccez.keys.learn.Review.later(service, f.lang, f.word); buzz(HapticFeedbackConstants.CONFIRM); review(f.lang to f.word) }
    /** Another word (a swipe). */
    fun reviewNext() = ui.review?.let { f -> ui.review = null; review(f.lang to f.word) }

    /** After a recovered error: the word as typed so far, and a clean slate. */
    fun recover() {
        runCatching { ic?.finishComposingText() }
        clearWord()
        fix = null
        autoSpace = false
        runCatching { refresh(); updateShift(); updateBar() }
    }

    fun finish() {
        Stats.flush()
        ui.review = null
        reviewSeq++
        rememberLang()
        // What's composing stays as typed (it'd be replaced by the next field's text otherwise).
        runCatching { if (conv.isNotEmpty() || romaji.isNotEmpty()) commitConv() else if (word.isNotEmpty()) ic?.finishComposingText() }
        clearWord()
        fix = null
        autoSpace = false
        swiped = null
        engine.save()
    }

    /** Layout, size and labels again (after a setting or language change). */
    fun refresh() {
        // `keys use` may have changed it.
        if (lang != engine.currentLanguage() && word.isEmpty() && conv.isEmpty() && romaji.isEmpty()) {
            lang = engine.currentLanguage()
        }
        // The converter is several MB: loaded off the main thread before the first kana.
        if (converting()) lang.let { l -> app.scope.launch { uk.ccez.keys.learn.Dictionaries.ensure(l); engine.convert(l, "") } }
        val face = app.flag("buttons.face")
        var layout = withoutFace(engine.layout(lang, page), !face)
        if (page == Page.LETTERS && app.flag("typing.numbers")) layout = withNumbers(layout)
        view.layout = layout
        view.scale = (engine.setting("keyboard.height").toIntOrNull() ?: 100) / 100f
        view.swipe = app.flag("typing.swipe") && !converting()
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
        if (app.flag("typing.sound")) {
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
        if (!app.flag("typing.vibrate")) return
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

    override fun onHoldSymbols() {
        if (lang != "ja" && lang != "zh") return
        buzz(HapticFeedbackConstants.LONG_PRESS)
        if (conv.isNotEmpty() || romaji.isNotEmpty()) commitConv()
        ui.panel = Panel.DRAW
    }

    /** A drawn character picked: in it goes. */
    fun putDrawn(text: String) {
        acted()
        ic?.commitText(text, 1)
        updateBar()
    }

    /** The last swiped word, while a pick from the bar may still replace it. */
    private var swiped: String? = null

    override fun onSwipe(path: String) {
        if (ui.ask != null || password || rawKeys) return
        val words = engine.swipe(lang, path)
        if (words.isEmpty()) return
        val ic = ic ?: return
        acted()
        endWord(correct = true)
        val before = ic.getTextBeforeCursor(1, 0)?.toString().orEmpty()
        var w = words[0]
        if (shift != Shift.OFF) w = w.replaceFirstChar { it.uppercase() }
        val prev = prevWord(0)
        ic.commitText((if (before.isNotEmpty() && !before[0].isWhitespace()) " " else "") + w + " ", 1)
        if (!noLearn) learn(lang, prev, w, false)
        lastSpaceAt = SystemClock.uptimeMillis()
        swiped = w
        if (shift == Shift.ONCE) setShift(Shift.OFF)
        noticeLanguage(w)
        // The strip, tips and capitals as after any word; then the swipe's other words in the bar.
        updateShift()
        updateBar()
        ui.bar = Bar.Words(words.getOrNull(1), w, words.getOrNull(2), false, null)
        autoSpace = true
    }

    override fun onRepeat(key: Key, heldMs: Long) {
        acted()
        deleteWord()
        buzz(HapticFeedbackConstants.CLOCK_TICK)
    }

    /** Back to the start of the word (or the run of marks) before the cursor. */
    private fun deleteWord() {
        ui.ask?.let { a ->
            val t = a.input.trimEnd()
            val keep = t.dropLastWhile(::isWordChar).let { if (it.length == t.length) t.dropLast(1) else it }
            ui.ask = a.copy(input = keep)
            return
        }
        val ic = ic ?: return
        fix = null
        autoSpace = false
        if (conv.isNotEmpty() || romaji.isNotEmpty()) {
            clearWord()
            ic.setComposingText("", 1)
            ui.bar = Bar.Buttons
        } else if (word.isNotEmpty()) {
            clearWord()
            ic.setComposingText("", 1)
        } else {
            val before = ic.getTextBeforeCursor(64, 0)?.toString().orEmpty()
            val n = uk.ccez.keys.core.deleteSpan(before).toInt()
            if (n > 0) ic.deleteSurroundingTextInCodePoints(n, 0)
        }
        updateShift()
        updateBar()
    }

    override fun onKey(key: Key) {
        val t0 = System.nanoTime()
        try { handleKey(key) } finally { Stats.key(System.nanoTime() - t0) }
    }

    private fun handleKey(key: Key) {
        acted()
        ui.tools = false
        swiped = null
        if (key.kind != KeyKind.BACKSPACE) romajiSwitch = null
        if (key.kind != KeyKind.BACKSPACE) fix = null
        when (key.kind) {
            KeyKind.CHAR -> type(if (shift != Shift.OFF && key.alt.isNotEmpty()) key.alt else key.text)
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

    private fun appLang(pkg: String?): String? =
        pkg?.let { service.getSharedPreferences("app-languages", android.content.Context.MODE_PRIVATE).getString(it, null) }

    private fun rememberLang() {
        val pkg = editor?.packageName ?: return
        service.getSharedPreferences("app-languages", android.content.Context.MODE_PRIVATE).edit().putString(pkg, lang).apply()
    }

    fun polite() = ui.polite

    /** Hold 🔊: this app's translations switch between casual and polite. */
    fun flipRegister() {
        val pkg = editor?.packageName ?: return
        val l = app.strings("apps.polite")
        val next = if (pkg in l) l - pkg else l + pkg
        engine.setSetting("apps.polite", org.json.JSONArray(next).toString())
        app.settingsChanged()
        buzz(HapticFeedbackConstants.CONTEXT_CLICK)
        ui.polite = pkg in next
        updateBar()
    }

    override fun onLanguage(code: String) {
        view.auto = false
        if (code == lang) return
        if (code == "ja" || code == "zh") app.scope.launch { uk.ccez.keys.learn.Dictionaries.ensure(code) }
        buzz(HapticFeedbackConstants.CONTEXT_CLICK)
        endWord(correct = false)
        engine.setCurrentLanguage(code)
        lang = code
        recent.clear()
        app.scope.launch { app.ensureWords(lang) }
        refresh()
        updateBar()
    }

    override fun onSpaceSwipe(step: Int) {
        view.auto = false
        buzz(HapticFeedbackConstants.CONTEXT_CLICK)
        endWord(correct = false)
        lang = engine.stepLanguage(step)
        recent.clear()
        app.scope.launch { app.ensureWords(lang) }
        refresh()
        updateBar()
    }

    private fun setPage(p: Page) {
        page = p
        refresh()
    }

    fun type(text: String) {
        acted()
        ui.ask?.let { a ->
            val t = if (shift != Shift.OFF) text.uppercase() else text
            ui.ask = a.copy(input = a.input + t)
            if (shift == Shift.ONCE) setShift(Shift.OFF)
            return
        }
        val ic = ic ?: return
        if (rawKeys) {
            val t = if (shift != Shift.OFF) text.uppercase() else text
            cached?.forget()
            t.forEach { service.sendKeyChar(it) }
            if (shift == Shift.ONCE) setShift(Shift.OFF)
            return
        }
        if (converting() && !noCorrect && text.length == 1 && text[0].isLetter() && text[0].code < 128) {
            autoSpace = false
            if (lang == "ja") {
                val r = engine.romaji(romaji + text.lowercase(), false)
                conv.append(r.kana)
                romaji = r.pending
            } else {
                conv.append(text.lowercase())
            }
            if (shift == Shift.ONCE) setShift(Shift.OFF)
            updateBar()
            return
        }
        if (converting() && (conv.isNotEmpty() || romaji.isNotEmpty())) commitConv()
        if (telexOn() && !noCorrect && text.length == 1 && text[0].isLetter() && text[0].code < 128) {
            autoSpace = false
            if (telexRaw.isEmpty() && word.isNotEmpty()) telexRaw.append(word)
            telexRaw.append(if (shift != Shift.OFF) text.uppercase() else text)
            word.setLength(0)
            word.append(engine.telex(telexRaw.toString()))
            ic.setComposingText(word, 1)
            if (shift == Shift.ONCE) setShift(Shift.OFF)
            updateBar()
            return
        }
        if (text.length == 1 && Hangul.isJamo(text[0]) && !password) {
            autoSpace = false
            if (jamo.isEmpty()) jamoBase = word.toString()
            jamo.append(text)
            word.setLength(0)
            word.append(jamoBase).append(Hangul.compose(jamo))
            ic.setComposingText(word, 1)
            if (shift == Shift.ONCE) setShift(Shift.OFF)
            updateBar()
            return
        }
        if (autoSpaceSwap(text, ic)) return
        if (text.length <= 2 && text.all(::isWordChar) && !password && !numeric) {
            val t = if (shift != Shift.OFF) text.uppercase() else text
            // A non-letter amid Korean or Telex letters: what's shown so far is the word now.
            jamo.setLength(0)
            jamoBase = ""
            telexRaw.setLength(0)
            if (word.isEmpty()) resumeWord(ic)
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

    /**
     * Letters typed right after a word (the cursor put back at its end) go on
     * with it: "swim" + "m" is "swimm", and the bar offers "swimming".
     */
    private fun resumeWord(ic: InputConnection) {
        if (noSpaces()) return
        val before = ic.getTextBeforeCursor(40, 0)?.toString().orEmpty()
        val tail = before.takeLastWhile(::isWordChar)
        if (tail.isEmpty() || tail.length >= 40) return
        // Not inside a word (letters after the cursor too): that stays as it is.
        val after = ic.getTextAfterCursor(1, 0)?.toString().orEmpty()
        if (after.isNotEmpty() && isWordChar(after[0])) return
        ic.deleteSurroundingText(tail.length, 0)
        word.append(tail)
    }

    /** After a word we spaced ourselves, ".,!?" go before that space. */
    private fun autoSpaceSwap(text: String, ic: InputConnection): Boolean {
        if (!autoSpace || word.isNotEmpty()) { autoSpace = false; return false }
        autoSpace = false
        val swap = uk.ccez.keys.core.markAfterSpace(lang, text)
        if (swap != null) {
            if (ic.getTextBeforeCursor(1, 0)?.toString() == " ") {
                ic.deleteSurroundingText(1, 0)
                ic.commitText(swap, 1)
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
        // Kana or pinyin not yet converted goes in as typed.
        if (conv.isNotEmpty() || romaji.isNotEmpty()) { commitConv(); return null }
        if (word.isEmpty()) return null
        val ic = ic ?: return null
        val (head, typed) = lastWordOf(word.toString())
        val prev = if (head.isNotEmpty()) lastWordOf(head).second else prevWord(word.length)
        val fixed = if (correct && !noCorrect) suggestFor(typed).correction?.let { head + it } else null
        val out = fixed ?: (head + typed)
        ic.commitText(out, 1)
        fix = fixed?.let { (head + typed) to it }
        clearWord()
        // What was typed tells the language, not what it became; the word is
        // learned in the language it turned out to be ("chez" in French, never
        // in English, where it would stop counting as foreign).
        noticeLanguage(typed)
        if (!noLearn) learn(lang, prev, lastWordOf(out).second, false)
        return out
    }

    /** Japanese noticed in romaji: the language before and the word as typed, while backspace can undo it. */
    private var romajiSwitch: Pair<String, String>? = null

    /** "konnichiha" typed on another keyboard: on to Japanese, the word as kana to convert. */
    private fun toJapanese(kana: String, typed: String) {
        val before = lang
        clearWord()
        lang = "ja"
        engine.setCurrentLanguage("ja")
        recent.clear()
        view.auto = true
        refresh()
        conv.append(kana)
        romajiSwitch = before to typed
        updateBar()
        app.log("info", "romaji-noticed", "from" to before)
    }

    /** Backspace right after: back to the language before, the word as typed and kept. */
    private fun undoJapanese(): Boolean {
        val (before, typed) = romajiSwitch ?: return false
        romajiSwitch = null
        if (romaji.isNotEmpty() || conv.toString() != engine.romaji(typed, true).kana) return false
        val ic = ic ?: return false
        clearWord()
        lang = before
        engine.setCurrentLanguage(before)
        view.auto = false
        refresh()
        word.append(typed)
        ic.setComposingText(word, 1)
        if (!noLearn) learn(lang, prevWord(typed.length), typed, true)
        buzz(HapticFeedbackConstants.CONTEXT_CLICK)
        updateBar()
        return true
    }

    /** The last suggestions asked for: a word's are asked while typing it and again as it ends. */
    private var lastSuggest: Triple<String, String, uk.ccez.keys.core.Suggestions>? = null

    private fun suggestFor(typed: String): uk.ccez.keys.core.Suggestions =
        lastSuggest?.takeIf { it.first == lang && it.second == typed }?.third
            ?: engine.suggest(lang, typed).also { lastSuggest = Triple(lang, typed, it) }

    /** What the core learns changes its suggestions: the last ones are dropped. */
    private fun learn(l: String, prev: String, w: String, strong: Boolean) {
        lastSuggest = null
        engine.learn(l, prev, w, strong)
    }

    /** The last words typed in this field, for noticing their language. */
    private val recent = ArrayDeque<String>()

    private fun noticeLanguage(w: String) {
        if (password || !w.any { it.isLetter() }) return
        recent.addLast(w)
        while (recent.size > 2) recent.removeFirst()
        // One clearly foreign word, else the last two together.
        val to = engine.detectLanguage(lang, listOf(w)) ?: engine.detectLanguage(lang, recent.toList()) ?: return
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
        if (rawKeys) { cached?.forget(); service.sendKeyChar(' '); return }
        if (converting() && (conv.isNotEmpty() || romaji.isNotEmpty())) {
            (ui.bar as? Bar.Convert)?.words?.firstOrNull()?.let { pickConversion(it) } ?: run { commitConv(); updateBar() }
            return
        }
        val now = SystemClock.uptimeMillis()
        if (word.isNotEmpty() && !noCorrect && !password) {
            engine.romajiWord(lang, word.toString())?.let { kana -> toJapanese(kana, word.toString()); return }
        }
        if (word.isNotEmpty()) {
            endWord(correct = true)
            ic.commitText(" ", 1)
        } else {
            val before = ic.getTextBeforeCursor(2, 0)?.toString().orEmpty()
            if (app.flag("typing.double_space_period") && now - lastSpaceAt < 1000 && uk.ccez.keys.core.spaceMakesStop(before)) {
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
        if (undoJapanese()) return
        if (converting() && (conv.isNotEmpty() || romaji.isNotEmpty())) {
            if (romaji.isNotEmpty()) romaji = romaji.dropLast(1) else conv.setLength(conv.length - 1)
            showConv()
            if (conv.isEmpty() && romaji.isEmpty()) updateBar()
            return
        }
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
                if (!noLearn) learn(lang, prevWord(typed.length), typed, true)
                buzz(HapticFeedbackConstants.CONTEXT_CLICK)
                updateBar()
                return
            }
        }
        if (telexRaw.isNotEmpty()) {
            telexRaw.setLength(telexRaw.length - 1)
            word.setLength(0)
            word.append(engine.telex(telexRaw.toString()))
            ic.setComposingText(word, 1)
            if (word.isEmpty()) updateShift()
            updateBar()
            return
        }
        if (jamo.isNotEmpty()) {
            jamo.setLength(jamo.length - 1)
            word.setLength(0)
            word.append(jamoBase).append(Hangul.compose(jamo))
            if (jamo.isEmpty()) jamoBase = ""
            ic.setComposingText(word, 1)
            updateBar()
            return
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
                cached?.forget()
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
        if (converting() && (conv.isNotEmpty() || romaji.isNotEmpty())) { commitConv(); updateBar(); return }
        endWord(correct = false)
        val info = editor
        val action = (info?.imeOptions ?: 0) and EditorInfo.IME_MASK_ACTION
        val noAction = (info?.imeOptions ?: 0) and EditorInfo.IME_FLAG_NO_ENTER_ACTION != 0
        val multiLine = (info?.inputType ?: 0) and InputType.TYPE_TEXT_FLAG_MULTI_LINE != 0
        if (!noAction && !multiLine && action != EditorInfo.IME_ACTION_NONE && action != EditorInfo.IME_ACTION_UNSPECIFIED) {
            ic.performEditorAction(action)
        } else {
            cached?.forget()
            service.sendKeyChar('\n')
        }
        updateShift()
        updateBar()
    }

    // --- the bar ---

    /** A cell tapped: that word, then a space. */
    fun pick(text: String, isTyped: Boolean) {
        val ic = ic ?: return
        acted()
        // After a swipe: the bar's other words replace the one put in.
        swiped?.let { w ->
            swiped = null
            val tail = "$w "
            if (ic.getTextBeforeCursor(tail.length, 0)?.toString() == tail) {
                ic.deleteSurroundingText(tail.length, 0)
                ic.commitText("$text ", 1)
                // The word picked is the one meant (the swiped one was learned already).
                if (!noLearn) learn(lang, prevWord(text.length + 1), text, true)
                updateBar()
                return
            }
        }
        // Thai: the pick takes the last word's place; the words before it stay.
        val head = lastWordOf(word.toString()).first
        val prev = if (head.isNotEmpty()) lastWordOf(head).second else prevWord(word.length)
        // A prediction right after a word (no space yet) gets one; Japanese,
        // Chinese and Thai take none.
        val gap = if (word.isEmpty() && !noSpaces()) ic.getTextBeforeCursor(1, 0)?.toString()?.lastOrNull()?.takeIf(::isWordChar)?.let { " " } ?: "" else ""
        ic.commitText(if (noSpaces()) head + text else "$gap$text ", 1)
        clearWord()
        fix = null
        if (!noLearn) learn(lang, prev, text, isTyped)
        lastSpaceAt = SystemClock.uptimeMillis()
        autoSpace = !noSpaces()
        updateShift()
        updateBar()
    }

    /** A screenshot from the clipboard panel: into the app as an image, or onto the clipboard. */
    fun sendShot(uri: android.net.Uri): Boolean {
        val ic = ic ?: return false
        val ed = editor ?: return false
        acted()
        endWord(correct = false)
        return runCatching { uk.ccez.keys.learn.Shots.send(service, ic, ed, uri) }.getOrDefault(false)
    }

    fun paste(text: String) {
        acted()
        endWord(correct = false)
        ic?.commitText(text, 1)
        updateShift()
        updateBar()
    }

    fun insertFace(text: String) {
        acted()
        endWord(correct = false)
        val before = ic?.getTextBeforeCursor(1, 0)?.toString().orEmpty()
        // A kaomoji wants a space before it, an emoji doesn't.
        val gap = if (text.length > 2 && before.isNotEmpty() && !before[0].isWhitespace()) " " else ""
        ic?.commitText(gap + text, 1)
        uk.ccez.keys.ui.Faces.used(service, text)
        updateBar()
    }

    fun currentLanguage() = lang

    fun openCamera() {
        endWord(correct = false)
        service.startActivity(android.content.Intent(service, CameraActivity::class.java).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    /** For the face panel's own bottom row. */
    fun press(kind: KeyKind) {
        acted()
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
        if (converting() && (conv.isNotEmpty() || romaji.isNotEmpty())) {
            // The strip too, while Japanese or Chinese is still being converted.
            strip.update(if (password) "" else sentence(), lang, allowed = !password, polite = polite(), ai = !noLearn)
            showConv()
            return
        }
        val sentence = if (password) "" else sentence()
        strip.update(sentence, lang, allowed = !password, polite = polite(), ai = !noLearn)
        tips.update(sentence, lang, allowed = !password && !noLearn, ended = sentence.trimEnd().lastOrNull()?.let { it in ".!?。！？" } == true)
        if (password) { ui.bar = Bar.Buttons; return }
        updateWords()
        ui.face = if (ui.chatApp && app.flag("buttons.face")) uk.ccez.keys.learn.Kaomoji.suggest(service, sentence, uk.ccez.keys.ui.Faces.recent(service)) else null
    }

    private fun updateWords() {
        if (word.isNotEmpty()) {
            val typed = lastWordOf(word.toString()).second
            val s = suggestFor(typed)
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
        val auto = app.flag("typing.capitals") && word.isEmpty() && !password &&
            (ic?.getCursorCapsMode(info.inputType) ?: 0) != 0
        setShift(if (auto) Shift.ONCE else Shift.OFF)
    }

    /** The app's selection now, when it can say (news of it arrives late, after the next keys). */
    private fun selectionNow(): Pair<Int, Int>? {
        val ic = ic ?: return null
        if (Build.VERSION.SDK_INT >= 33) {
            runCatching { ic.takeSnapshot() }.getOrNull()?.let { return it.selectionStart to it.selectionEnd }
        }
        val et = runCatching { ic.getExtractedText(android.view.inputmethod.ExtractedTextRequest().apply { hintMaxChars = 0 }, 0) }.getOrNull() ?: return null
        return (et.startOffset + et.selectionStart) to (et.startOffset + et.selectionEnd)
    }

    fun onSelection(newStart: Int, newEnd: Int, candStart: Int, candEnd: Int) {
        // The text may have changed without us (the app, another cursor): read it afresh.
        cached?.forget()
        val now = selectionNow()
        // Old news (from an edit before the last keys): a newer report is on its way.
        if (now != null && (now.first != newStart || now.second != newEnd)) return
        val ours = SystemClock.uptimeMillis() - actedAt < 400
        val composing = word.isNotEmpty() || conv.isNotEmpty() || romaji.isNotEmpty()
        if (composing) {
            if (candEnd != -1 && newStart == candEnd && newEnd == candEnd) return
            // Without a way to check, a report just after a key is taken as ours.
            if (now == null && ours) return
            // The cursor moved away from the word: keep it as typed.
            ic?.finishComposingText()
            clearWord()
        } else {
            if (ours || candStart != -1) return
        }
        // The cursor really moved: what was about the last word no longer applies.
        fix = null
        autoSpace = false
        swiped = null
        recent.clear()
        updateShift()
        updateBar()
    }

    /** The sentence being typed (the last one, when the cursor sits just after its end). */
    /** The sentence being typed (the rule is the core's: text.rs, tested there). */
    private fun sentence(): String = uk.ccez.keys.core.sentenceOf(ic?.getTextBeforeCursor(300, 0)?.toString().orEmpty())

    /** The sentence before the cursor swapped for `text` (held strip flag). */
    fun replaceSentence(text: String, of: String? = null) {
        val ic = ic ?: return
        acted()
        val s = sentence()
        if (s.isEmpty()) return
        // Only the translation of the sentence as it stands (not an older one still showing).
        if (of != null && of.trim() != s.trim()) return
        val before = ic.getTextBeforeCursor(300, 0)?.toString().orEmpty()
        val at = before.lastIndexOf(s)
        if (at < 0) return
        clearWord()
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
        if (!noLearn) engine.noteMistake(t.lang, t.sentence, t.fix.text, t.fix.why)
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

    /** The buzz of a hold. */
    fun holdBuzz() = buzz(HapticFeedbackConstants.LONG_PRESS)

    fun setStatus(lang: String, word: String, status: uk.ccez.keys.core.WordStatus) {
        if (noLearn) return
        strip.setStatus(lang, word, status)
    }

    fun engineStatus(lang: String, word: String) = engine.wordStatus(lang, word)


    /** The word before the cursor, skipping `skip` characters of the word being typed. */
    private fun prevWord(skip: Int): String {
        val before = ic?.getTextBeforeCursor(64 + skip, 0)?.toString().orEmpty()
        val t = before.dropLast(skip).trimEnd()
        if (noSpaces()) {
            // Japanese, Chinese, Thai: no spaces between words; ICU (in Android) finds the last one.
            if (t.isEmpty() || !isWordChar(t.last())) return ""
            val it = android.icu.text.BreakIterator.getWordInstance(android.icu.util.ULocale.forLanguageTag(lang))
            it.setText(t)
            val start = it.preceding(t.length).coerceAtLeast(0)
            return t.substring(start)
        }
        return uk.ccez.keys.core.wordBefore(before, skip.toUInt())
    }

    /**
     * Thai has no spaces between words, so what's being typed can run on for
     * several: ICU finds the last one, which suggestions and fixes are for.
     * Elsewhere the whole word is the last one.
     */
    private fun lastWordOf(w: String): Pair<String, String> {
        if (lang != "th" || w.length < 2) return "" to w
        val it = android.icu.text.BreakIterator.getWordInstance(android.icu.util.ULocale.forLanguageTag("th"))
        it.setText(w)
        val start = it.preceding(w.length).coerceAtLeast(0)
        return w.substring(0, start) to w.substring(start)
    }

    /** Languages written without spaces between words. */
    private fun noSpaces() = lang == "ja" || lang == "zh" || lang == "th"

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

        fun isWordChar(c: Char) = c.isLetterOrDigit() || c == '\'' || c == '’' || c == '-' || isMark(c)

        /** Vowel signs and other marks (Hindi ि, Thai ิ) and the joiners: part of the word. */
        private fun isMark(c: Char) = c == '\u200C' || c == '\u200D' || when (Character.getType(c).toByte()) {
            Character.NON_SPACING_MARK, Character.COMBINING_SPACING_MARK, Character.ENCLOSING_MARK -> true
            else -> false
        }

        fun keysHeightDp(scale: Float, rows: Int = 4) = 6 + 6 + rows * 44 * scale + (rows - 1) * 8

        /** SwiftKey's row of digits over the letters (Settings: Numbers). */
        private fun withNumbers(l: Layout): Layout {
            val digits = "1234567890".map { d -> Key(KeyKind.CHAR, d.toString(), d.toString(), "", "", emptyList(), 1f) }
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
