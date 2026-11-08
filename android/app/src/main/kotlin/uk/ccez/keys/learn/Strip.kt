package uk.ccez.keys.learn

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
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
)

/** A word under a suggestion, in a language being learned. */
data class Gloss(val lang: String, val flag: String, val word: String)

/** A tapped word's card. */
data class CardView(val lang: String, val word: String, val lemma: String, val card: Card?, val meaning: String?)

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
    private var glossJob: Job? = null
    private var index = 0
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
    /** AI wordings already fetched: (from, to, polite, text) → translation. */
    private val refined = object : LinkedHashMap<String, String>(32, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, String>?) = size > 64
    }

    fun update(sentence: String, typingLang: String, allowed: Boolean, polite: Boolean = false) {
        this.polite = polite
        if (!allowed || !engine.settingBool("learning.strip") || sentence.isBlank() || !Translator.supports(typingLang)) {
            if (sentence.isBlank()) index = 0
            job?.cancel()
            ui.strip = null
            return
        }
        source = sentence
        typing = typingLang
        job?.cancel()
        job = app.scope.launch {
            delay(180)
            show(autoSteps = 0)
        }
    }

    /** The next (1) or previous (-1) language. */
    fun step(by: Int) {
        val n = targets(typing).size
        if (n == 0) return
        index = ((index + by) % n + n) % n
        job?.cancel()
        job = app.scope.launch { show(autoSteps = n) }
    }

    private suspend fun show(autoSteps: Int) {
        val all = targets(typing)
        if (all.isEmpty()) return
        index %= all.size
        val lang = all[index]
        // A dictionary that arrives now brings readings and statuses: redraw.
        app.scope.launch { if (Dictionaries.ensure(lang) && ui.strip?.lang == lang) redraw() }
        val key = "$typing|$lang|$polite|$source"
        val text = synchronized(refined) { refined[key] } ?: Translator.translate(source, typing, lang) ?: return
        refine(key, source, typing, lang)
        val words = engine.stripWords(lang, text)
        val flags = all.map { Langs.of(app, it)?.flag ?: it }
        val next = (1..minOf(3, all.size - 1)).map { flags[(index + it) % all.size] }
        withContext(Dispatchers.Main) { ui.strip = StripView(lang, flags[index], next, words, text) }
        val known = words.filter { it.word }
        if (known.isNotEmpty() && known.all { it.status == WordStatus.KNOWN } && autoSteps < all.size - 1) {
            delay(1500)
            index = (index + 1) % all.size
            show(autoSteps + 1)
        }
    }

    /**
     * Once typing pauses, the AI's wording (natural, in the app's register)
     * replaces ML Kit's literal one.
     */
    private fun refine(key: String, source: String, from: String, to: String) {
        refineJob?.cancel()
        if (!engine.settingBool("learning.refine") || synchronized(refined) { refined.containsKey(key) }) return
        val aiKey = Secrets.get(app) ?: return
        val polite = polite
        refineJob = app.scope.launch {
            delay(1200)
            val t = runCatching { engine.translateAi(source, from, to, polite, aiKey) }
                .onFailure { app.log("warn", "refine-failed", "error" to it.message) }
                .getOrNull()?.takeIf { it.isNotBlank() } ?: return@launch
            synchronized(refined) { refined[key] = t }
            if (ui.strip?.lang == to && this@Strip.source == source) redraw()
        }
    }

    /** The bold suggestion in up to two languages being learned, skipping ones where it's known. */
    fun gloss(word: String?, typingLang: String) {
        glossJob?.cancel()
        if (word == null || !engine.settingBool("learning.strip") || !Translator.supports(typingLang)) { ui.glosses = emptyList(); return }
        glossJob = app.scope.launch {
            delay(250)
            val out = mutableListOf<Gloss>()
            for (lang in targets(typingLang).take(4)) {
                if (out.size == 2) break
                val t = Translator.translate(word, typingLang, lang)?.trim()?.trimEnd('.', '!', '?') ?: continue
                if (t.isEmpty() || t.equals(word, ignoreCase = true) || t.count { it == ' ' } > 2) continue
                if (engine.wordStatus(lang, t) == WordStatus.KNOWN) continue
                out += Gloss(lang, Langs.of(app, lang)?.flag ?: lang, t)
            }
            withContext(Dispatchers.Main) { ui.glosses = out }
        }
    }

    fun open(lang: String, w: StripWord) {
        app.scope.launch {
            val card = engine.wordCard(lang, w.text, w.lemma)
            val meaning = if (card?.meanings.isNullOrEmpty()) Translator.translate(w.lemma.ifEmpty { w.text }, lang, defaultLang()) else null
            withContext(Dispatchers.Main) { ui.card = CardView(lang, w.text, w.lemma, card, meaning) }
        }
    }

    fun setStatus(lang: String, word: String, status: WordStatus) {
        engine.setWordStatus(lang, word, status)
        WordSync.soon()
        redraw()
    }

    /** The same language again, without moving on. */
    private fun redraw() {
        job?.cancel()
        job = app.scope.launch { show(autoSteps = Int.MAX_VALUE / 2) }
    }
}
