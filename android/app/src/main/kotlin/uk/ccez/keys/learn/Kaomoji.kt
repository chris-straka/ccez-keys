package uk.ccez.keys.learn

import android.content.Context
import org.json.JSONObject

/**
 * In chat apps, a kaomoji when the message fits a mood (data/kaomoji-triggers.json):
 * the one used most lately from that mood, else its first.
 */
object Kaomoji {
    /** Per mood: one pattern for all its words (compiled once), and its faces. */
    private var moods: List<Pair<Regex, List<String>>> = emptyList()

    private fun load(ctx: Context) {
        if (moods.isNotEmpty()) return
        val faces = JSONObject(ctx.assets.open("faces.json").bufferedReader().readText()).getJSONArray("kaomoji")
        val t = JSONObject(ctx.assets.open("kaomoji-triggers.json").bufferedReader().readText())
        val names = t.getJSONArray("moods")
        val words = t.getJSONObject("words")
        moods = (0 until names.length()).map { i ->
            val w = words.getJSONArray(names.getString(i)).let { a -> (0 until a.length()).map { a.getString(it).lowercase() } }
            val f = faces.getJSONArray(i).let { a -> (1 until a.length()).map(a::getString) }
            // Words match whole ("thanks", not inside "thanksgiving"); marks like ":)" anywhere.
            val alts = w.map { x -> if (x.all { it.isLetter() || it == '\'' || it == ' ' }) "(?<![\\p{L}])" + Regex.escape(x) + "(?![\\p{L}])" else Regex.escape(x) }
            Regex(alts.joinToString("|")) to f
        }
    }

    /** The face for `sentence`, or null. `recent`: faces used, newest first. */
    fun suggest(ctx: Context, sentence: String, recent: List<String>): String? {
        if (sentence.isBlank()) return null
        load(ctx)
        val s = " " + sentence.lowercase() + " "
        // The mood whose word comes last wins.
        var best: Pair<Int, List<String>>? = null
        for ((pattern, faces) in moods) {
            val at = pattern.findAll(s).lastOrNull()?.range?.first ?: continue
            if (best == null || at > best.first) best = at to faces
        }
        val faces = best?.second ?: return null
        return recent.firstOrNull { it in faces } ?: faces.firstOrNull()
    }
}
