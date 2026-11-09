package uk.ccez.keys.learn

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uk.ccez.keys.App
import uk.ccez.keys.UiState
import uk.ccez.keys.core.Card
import uk.ccez.keys.core.StripWord
import uk.ccez.keys.core.WordStatus
import uk.ccez.keys.ui.Langs

/** What the learning strip shows. */
data class StripView(
    val lang: String,
    val flag: String,
    /** The next languages' flags, small, under the current one. */
    val next: List<String>,
    val words: List<StripWord>,
    val text: String,
    /** The AI's wording (else ML Kit's quick one). */
    val refined: Boolean = false,
    /** The AI is rewording it now. */
    val pending: Boolean = false,
    /** The sentence it translates. */
    val source: String = "",
    /** How it got this language: 1 on down the owner's list, -1 back up (for the slide). */
    val moved: Int = 1,
    /** The same in the next languages, as many as fit ("chat | Katze | gato"). */
    val more: List<Segment> = emptyList(),
)

/** One language's translation of a single word, after the first. */
data class Segment(val lang: String, val flag: String, val words: List<StripWord>, val text: String)

/** A tapped word's card. */
data class CardView(
    val lang: String,
    val word: String,
    val lemma: String,
    val card: Card?,
    val meaning: String?,
    val reading: String = "",
    /** Muse: what it means in this sentence, and at most one short note. */
    val here: String? = null,
    val note: String? = null,
    /** Muse is being asked. */
    val asking: Boolean = false,
)

/** Without punctuation left at the end ("テスト・", "chat."). */
private fun bare(t: String) = t.trimEnd { it.isWhitespace() || Character.getType(it).toByte().let { k -> k == Character.OTHER_PUNCTUATION || k == Character.DASH_PUNCTUATION || k == Character.CONNECTOR_PUNCTUATION } }

/**
 * The learning strip: the sentence being typed, in a language being
 * learned (or, typing in one of those, in the default language). A
 * sentence whose words are all known shows grey for a moment, then the
 * next language takes over.
 */
class Strip(private val ui: UiState) {
    private val app = App.instance
    private val engine get() = app.engine
    private var job: Job? = null
    private var index = 0
    /** The way the last change of language went (1 down the list, -1 up). */
    private var moved = 1
    private var source = ""
    private var typing = ""

    private fun defaultLang() = engine.setting("languages.default").trim('"')

    /** Where the strip translates to, in turn. */
    private fun targets(typing: String): List<String> {
        val def = defaultLang()
        val learning = engine.settingStrings("languages.learning")
        return if (typing != def && typing in learning) listOf(def)
        else learning.filter { it != typing && Translator.supports(it) }
    }

    private var polite = false
    private var refineJob: Job? = null
    /**
     * AI wordings already fetched: (from, to, polite, text) → translation. Kept
     * on disk, so going back to a sentence (or the keyboard restarting) never
     * asks Muse twice.
     */
    private val refined = object : LinkedHashMap<String, String>(64, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, String>?) = size > 500
    }
    private val refinedFile by lazy { java.io.File(app.filesDir, "refined.json") }
    private var saveJob: Job? = null

    init {
        app.scope.launch {
            runCatching {
                val o = org.json.JSONObject(refinedFile.readText())
                synchronized(refined) { o.keys().forEach { k -> if (!refined.containsKey(k)) refined[k] = o.getString(k) } }
            }
        }
    }

    private fun remember(key: String, value: String) {
        synchronized(refined) { refined[key] = value }
        saveJob?.cancel()
        saveJob = app.scope.launch {
            delay(2000)
            val text = synchronized(refined) { org.json.JSONObject(refined as Map<*, *>).toString() }
            val tmp = java.io.File(refinedFile.path + ".tmp")
            runCatching { tmp.writeText(text); tmp.renameTo(refinedFile) }
        }
    }

    /** `ai`: the AI may reword it (never in a field that asks for no learning). */
    private var ai = true

    fun update(sentence: String, typingLang: String, allowed: Boolean, polite: Boolean = false, ai: Boolean = true) {
        this.polite = polite
        this.ai = ai
        if (!allowed || !engine.settingBool("learning.strip") || sentence.isBlank() || !Translator.supports(typingLang)) {
            if (sentence.isBlank()) index = 0
            job?.cancel()
            ui.strip = null
            return
        }
        source = sentence
        job?.cancel()
        job = app.scope.launch {
            delay(180)
            // The sentence's own language (the keyboard may be on another):
            // English typed on Français still shows the learning languages.
            typing = written(sentence, typingLang)
            show(autoSteps = 0)
        }
    }

    /** One language-ID client for the keyboard's life (each one holds native memory). */
    private val langId by lazy { com.google.mlkit.nl.languageid.LanguageIdentification.getClient() }

    /** The language `sentence` is in, among those typed; the keyboard's when unsure. */
    private suspend fun written(sentence: String, keyboard: String): String {
        // One word: the keyboard's language (it switches itself when the word is clearly another's).
        if (sentence.length < 4 || sentence.none { it.isWhitespace() }) return keyboard
        val typed = engine.typingLanguages()
        val found = runCatching { langId.identifyPossibleLanguages(sentence).await() }.getOrNull().orEmpty()
        val best = found.filter { it.languageTag in typed }.maxByOrNull { it.confidence } ?: return keyboard
        return if (best.languageTag != keyboard && best.confidence >= 0.6f) best.languageTag else keyboard
    }

    /** The next (1) or previous (-1) language. */
    fun step(by: Int) {
        val n = targets(typing).size
        if (n == 0) return
        index = ((index + by) % n + n) % n
        moved = if (by < 0) -1 else 1
        job?.cancel()
        job = app.scope.launch { show(autoSteps = n) }
    }

    /** `pause`: how long an all-known sentence shows before the next language. */
    private suspend fun show(autoSteps: Int, pause: Long = 1500) {
        val all = targets(typing)
        if (all.isEmpty()) return
        index %= all.size
        val lang = all[index]
        // A dictionary that arrives now brings readings and statuses: redraw.
        app.scope.launch { if (Dictionaries.ensure(lang) && ui.strip?.lang == lang) redraw() }
        val key = "$typing|$lang|$polite|$source"
        val better = synchronized(refined) { refined[key] }
        val text = (better ?: Translator.translate(source, typing, lang) ?: return)
            .let { if (source.none { c -> c.isWhitespace() }) bare(it) else it }
        val pending = better == null && refine(key, source, typing, lang, all)
        val words = engine.stripWords(lang, text)
        val flags = all.map { Langs.of(app, it)?.flag ?: it }
        val next = emptyList<String>()
        withContext(Dispatchers.Main) {
            // The other languages stay as they were until their new wording is ready (no blink per key).
            val keep = ui.strip?.takeIf { it.lang == lang }?.more.orEmpty()
            ui.strip = StripView(lang, flags[index], next, words, text, refined = better != null, pending = false, source = source, moved = moved, more = keep)
        }
        if (pending) {
            // Shown as pending once the pause has come and the AI is asked.
            app.scope.launch {
                delay(1200)
                if (refineJob?.isActive != true) return@launch
                withContext(Dispatchers.Main) { ui.strip?.takeIf { it.text == text && !it.refined }?.let { ui.strip = it.copy(pending = true) } }
            }
        }
        prefetch(all)
        // The voice ready before a word is tapped.
        Speaker.warm(app, lang, text)
        val known = words.filter { it.word }
        if (known.isNotEmpty() && known.all { it.status == WordStatus.KNOWN } && autoSteps < all.size - 1) {
            delay(pause)
            index = (index + 1) % all.size
            moved = 1
            show(autoSteps + 1)
            return
        }
        addMore(all, lang, text)
    }

    private fun flag(lang: String) = Langs.of(app, lang)?.flag ?: lang

    /**
     * The sentence in the languages after the one shown, "… | Das Haus ist groß | …",
     * skipping those where it's all known; the line shows what fits.
     */
    private suspend fun addMore(all: List<String>, shownLang: String, shownText: String) {
        val src = source
        val segs = mutableListOf<Segment>()
        // About what the line holds, so languages that can't show aren't translated.
        // (A bit more than fits: the line itself drops what doesn't.)
        var room = 52 - shownText.length
        var warmed = false
        for (k in 1 until all.size) {
            if (room <= 0 || segs.size >= 5) break
            val lang = all[(index + k) % all.size]
            // Only models already on the phone: no downloads for these.
            val ai = synchronized(refined) { refined["$typing|$lang|$polite|$src"] }
            val got = ai ?: Translator.translate(src, typing, lang, download = false)
            if (got == null) {
                // Its model isn't on the phone yet: fetched quietly (one at a time, on Wi-Fi),
                // the line fills in after.
                if (!warmed) { warmed = true; Translator.warm(typing, lang) { if (source == src) redraw() } }
                continue
            }
            val t = bare(got)
            if (t.isEmpty() || t.equals(src, ignoreCase = true)) continue
            val words = engine.stripWords(lang, t)
            val ws = words.filter { it.word }
            if (ws.isNotEmpty() && ws.all { it.status == WordStatus.KNOWN }) continue
            segs += Segment(lang, flag(lang), words, t)
            room -= t.length + 4
        }
        // All at once: one change to the line, not one per language.
        if (segs.isEmpty()) return
        withContext(Dispatchers.Main) { ui.strip?.takeIf { it.source == src && it.lang == shownLang }?.let { ui.strip = it.copy(more = segs.toList()) } }
    }

    /** The languages either side, translated ahead so a swipe shows them at once. */
    private fun prefetch(all: List<String>) {
        if (all.size < 2) return
        val text = source
        val from = typing
        val n = all.size
        app.scope.launch {
            for (d in listOf(1, -1)) Translator.translate(text, from, all[((index + d) % n + n) % n], download = false)
        }
    }

    /**
     * Once typing pauses, the AI's wording (natural, in the app's register)
     * replaces ML Kit's literal one.
     */
    /** True when the AI will be asked. */
    private fun refine(key: String, source: String, from: String, to: String, all: List<String>): Boolean {
        refineJob?.cancel()
        if (!ai || !engine.settingBool("learning.refine") || synchronized(refined) { refined.containsKey(key) }) return false
        // Sentences, and single uncommon words ("gerrymandering": ML Kit made it a verb).
        // A common word or two keeps ML Kit's plain translation (owner, 2026-11-08:
        // "worth" became a whole phrase).
        val single = source.none { it.isWhitespace() }
        val rare = single && source.length >= 6 && !engine.commonWord(from, source)
        if (!rare) {
            if (source.split(Regex("\\s+")).count { it.isNotBlank() } < 3 && source.none { it in '\u3040'..'\u9fff' }) return false
            if (single && source.length < 6) return false
        }
        // A rare word: the languages after it on the line too, all replaced at once.
        val also = if (rare) (1 until minOf(all.size, 5)).map { all[(index + it) % all.size] } else emptyList()
        val aiKey = Secrets.get(app) ?: return false
        val polite = polite
        refineJob = app.scope.launch {
            delay(1200)
            // The other languages asked alongside (one wait, one change on the line).
            val others = also.filter { lang -> synchronized(refined) { !refined.containsKey("$from|$lang|$polite|$source") } }.map { lang ->
                async { lang to runCatching { uk.ccez.keys.Stats.ai.incrementAndGet(); engine.translateAi(source, from, lang, polite, aiKey) }.getOrNull()?.trim()?.takeIf { it.isNotEmpty() } }
            }
            val t = runCatching { uk.ccez.keys.Stats.ai.incrementAndGet(); engine.translateAi(source, from, to, polite, aiKey) }
                .onFailure { app.log("warn", "refine-failed", "error" to it.message) }
                .getOrNull()?.takeIf { it.isNotBlank() }
            if (t == null) {
                // Offline or failed: ML Kit's wording stays, without the pulse.
                withContext(Dispatchers.Main) { ui.strip?.takeIf { it.pending && it.source == source }?.let { ui.strip = it.copy(pending = false) } }
                return@launch
            }
            remember(key, t)
            others.awaitAll().forEach { (lang, v) -> if (v != null) remember("$from|$lang|$polite|$source", v) }
            if (ui.strip?.lang == to && this@Strip.source == source) redraw()
        }
        return true
    }

    fun open(lang: String, w: StripWord) {
        // The same word again: its card closes.
        ui.card?.let { c -> if (c.lang == lang && c.word == w.text) { ui.card = null; return } }
        // Heard as it opens.
        Speaker.say(app, w.text, lang)
        val said = source
        app.scope.launch {
            val card = engine.wordCard(lang, w.text, w.lemma)?.let { c -> c.copy(meanings = inContext(c.meanings, said)) }
            val meaning = if (card?.meanings.isNullOrEmpty()) Translator.translate(w.lemma.ifEmpty { w.text }, lang, defaultLang()) else null
            // Furigana or pinyin, now only on the card (owner, 2026-11-08: a shorter line).
            val reading = w.ruby.joinToString("") { it.rt.ifEmpty { it.text } }.takeIf { r -> w.ruby.any { it.rt.isNotEmpty() } }.orEmpty()
            // In this sentence: Muse's gloss (kept on disk), else asked now.
            val context = ui.strip?.takeIf { it.lang == lang }?.text ?: ui.strip?.more?.firstOrNull { it.lang == lang }?.text.orEmpty()
            val key = "gloss|$lang|${w.text}|$context"
            val known = synchronized(refined) { refined[key] }
            val aiKey = if (ai && context.isNotBlank() && engine.settingBool("learning.refine")) Secrets.get(app) else null
            fun split(t: String) = t.lines().map { it.trim() }.filter { it.isNotEmpty() }.let { it.firstOrNull() to it.drop(1).joinToString(" ").ifEmpty { null } }
            val (here, note) = known?.let(::split) ?: (null to null)
            val view = CardView(lang, w.text, w.lemma, card, meaning, reading, here, note, asking = known == null && aiKey != null)
            withContext(Dispatchers.Main) { ui.card = view }
            if (known != null || aiKey == null) return@launch
            val got = runCatching { uk.ccez.keys.Stats.ai.incrementAndGet(); engine.gloss(context, w.text, aiKey) }.getOrNull()?.takeIf { it.isNotBlank() }
            if (got != null) remember(key, got)
            withContext(Dispatchers.Main) {
                ui.card?.takeIf { it.lang == lang && it.word == w.text }?.let { c ->
                    val (h, n) = got?.let(::split) ?: (null to null)
                    ui.card = c.copy(here = h, note = n, asking = false)
                }
            }
        }
    }

    /**
     * The meanings that match a word of the sentence typed come first: "I read
     * a book" puts livre's "book" before "delivers".
     */
    private fun inContext(meanings: List<String>, said: String): List<String> {
        val words = said.lowercase().split(Regex("[^\\p{L}']+")).filter { it.length >= 3 }.toSet()
        if (words.isEmpty()) return meanings
        fun hits(m: String) = m.lowercase().split(Regex("[^\\p{L}']+")).count { it in words || words.any { w -> it.length >= 4 && (w.startsWith(it) || it.startsWith(w)) } }
        return meanings.withIndex().sortedWith(compareByDescending<IndexedValue<String>> { hits(it.value) > 0 }.thenBy { it.index }).map { it.value }
    }

    fun setStatus(lang: String, word: String, status: WordStatus) {
        engine.setWordStatus(lang, word, status)
        WordSync.soon()
        if (status == WordStatus.KNOWN && ui.strip?.lang == lang) {
            // Everything shown now known: on to the next language.
            job?.cancel()
            job = app.scope.launch { show(autoSteps = 0, pause = 300) }
        } else {
            redraw()
        }
    }

    /** The same language again, without moving on. */
    private fun redraw() {
        job?.cancel()
        job = app.scope.launch { show(autoSteps = Int.MAX_VALUE / 2) }
    }
}
