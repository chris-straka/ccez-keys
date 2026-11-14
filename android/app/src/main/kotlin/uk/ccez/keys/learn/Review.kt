package uk.ccez.keys.learn

import android.content.Context
import kotlinx.coroutines.withTimeoutOrNull
import uk.ccez.keys.App
import uk.ccez.keys.core.WordStatus
import uk.ccez.keys.ui.Langs

/**
 * Words to review, one at a time: the owner's Learning words (from every app,
 * through ccez-words) in a shuffled round kept in prefs, so each comes up.
 * The keyboard shows one in the strip on an empty field, answered easy,
 * medium or hard on the screen words' FSRS schedule (the core's review,
 * synced through ccez-words, so the Mac and the phone share it; owner,
 * 2026-11-14). (The prefs are still called "widget": they kept the
 * home-screen widget's round, replaced by the words on screen.)
 */
object Review {

    /** A word to show: its flag, article, reading, form tag and meaning. */
    data class Face(
        val lang: String,
        val word: String,
        val flag: String,
        val article: String,
        val reading: String,
        val form: String,
        val meaning: String,
    )

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("widget", Context.MODE_PRIVATE)
    /** "lang|word" → "due millis,step": words put off with the old Later, kept off until then. */
    private fun later(ctx: Context) = ctx.getSharedPreferences("widget-later", Context.MODE_PRIVATE)

    /** The Learning words due now. */
    fun due(ctx: Context): List<Pair<String, String>> {
        val now = System.currentTimeMillis()
        val off = later(ctx)
        val e = App.instance.engine
        return e.learningWords()
            .map { it.lang to it.word }
            .filter { (l, w) -> (off.getString("$l|$w", null)?.substringBefore(',')?.toLongOrNull() ?: 0L) <= now && e.reviewDue(l, w) }
    }

    /** The next word of the round (not `shown`, unless it's the only one), left in the round until [seen]; null with none due. */
    fun next(ctx: Context, shown: Pair<String, String>?): Pair<String, String>? = synchronized(this) {
        val words = due(ctx)
        if (words.isEmpty()) return null
        val p = prefs(ctx)
        val keys = words.map { "${it.first}|${it.second}" }.toSet()
        val now = shown?.let { "${it.first}|${it.second}" }
        var order = p.getString("order", "").orEmpty().split('\n').filter { it in keys }
        if (order.isEmpty() || (order.size == 1 && order[0] == now && keys.size > 1)) {
            order = keys.shuffled().let { l -> if (l.size > 1 && l.first() == now) l.drop(1) + l.first() else l }
            p.edit().putString("order", order.joinToString("\n")).apply()
        }
        val k = order.firstOrNull { it != now } ?: order.first()
        return k.split('|', limit = 2).let { it[0] to it[1] }
    }

    /** A word was put on screen: off the round until it comes round again (one never shown stays). */
    fun seen(ctx: Context, lang: String, word: String) = synchronized(this) {
        val p = prefs(ctx)
        val order = p.getString("order", "").orEmpty().split('\n').filter { it.isNotEmpty() && it != "$lang|$word" }
        p.edit().putString("order", order.joinToString("\n")).apply()
    }

    /** Known: known everywhere (synced), out of the round. */
    fun known(ctx: Context, lang: String, word: String) {
        App.instance.engine.setWordStatus(lang, word, WordStatus.KNOWN)
        WordSync.soon()
        later(ctx).edit().remove("$lang|$word").apply()
    }

    /** Easy, medium or hard: FSRS's Easy, Good and Again (hard comes back in minutes). Synced with the statuses. */
    fun grade(ctx: Context, lang: String, word: String, g: uk.ccez.keys.core.Grade) {
        App.instance.engine.gradeScreenWord(lang, word, g)
        later(ctx).edit().remove("$lang|$word").apply()
        WordSync.soon()
    }

    /** Articles a saved word may already start with ("l'ardoise", "la gare"): no second one goes before it. */
    private val ARTICLES = listOf("l'", "l’", "d'", "d’", "le ", "la ", "les ", "un ", "une ", "el ", "los ", "las ", "der ", "die ", "das ", "ein ", "eine ", "il ", "lo ", "gli ", "o ", "os ", "as ", "de ", "het ")

    /** The article to show before `word`: none when it already has one. */
    fun article(word: String, article: String): String =
        if (article.isEmpty() || ARTICLES.any { word.lowercase().startsWith(it) }) "" else article

    /** A meaning that only names its word's form ("past participle of unterhalten"). */
    private fun isForm(meaning: String) = uk.ccez.keys.core.formTag(meaning).isNotEmpty()

    /** How a word shows: from the dictionary, the phone's translator when it has nothing. Off the main thread. */
    suspend fun face(ctx: Context, lang: String, word: String): Face {
        val app = App.instance
        val card = runCatching { app.engine.wordCard(lang, word, "") }.getOrNull()
        val meanings = card?.meanings.orEmpty()
        val lemma = card?.lemma?.takeIf { it.isNotEmpty() && it != word }
        // As ccez-lang does: "past participle of …" is the form (a small tag),
        // the meaning (its word's, if need be) the line. Only a form of the
        // card's own word is tagged ("gare" is a word, though also a form of "garer").
        val pointer = lemma?.let { l -> meanings.firstOrNull { uk.ccez.keys.core.formTag(it).endsWith(" · $l") } }
        val meaning = meanings.firstOrNull { !isForm(it) }
            ?: lemma?.let { l -> runCatching { app.engine.wordCard(lang, l, "") }.getOrNull()?.meanings?.firstOrNull { !isForm(it) } }
            ?: withTimeoutOrNull(4000) { Translator.translate(word, lang, app.text("languages.default"), download = false) }
            ?: ""
        val article = article(word, card?.article.orEmpty())
        return Face(
            lang = lang,
            word = word,
            flag = Langs.of(ctx, lang)?.flag ?: lang,
            article = article,
            reading = card?.reading?.takeIf { it.isNotEmpty() && it != word }.orEmpty(),
            form = pointer?.let { uk.ccez.keys.core.formTag(it) }.orEmpty(),
            meaning = meaning,
        )
    }
}
