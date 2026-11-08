package uk.ccez.keys.learn

import android.content.Context
import org.json.JSONObject

/**
 * In chat apps, a kaomoji when the message fits a mood (data/kaomoji-triggers.json):
 * the one used most lately from that mood, else its first.
 */
object Kaomoji {
    private var moods: List<Pair<List<String>, List<String>>> = emptyList()

    private fun load(ctx: Context) {
        if (moods.isNotEmpty()) return
        val faces = JSONObject(ctx.assets.open("faces.json").bufferedReader().readText()).getJSONArray("kaomoji")
        val t = JSONObject(ctx.assets.open("kaomoji-triggers.json").bufferedReader().readText())
        val names = t.getJSONArray("moods")
        val words = t.getJSONObject("words")
        moods = (0 until names.length()).map { i ->
            val w = words.getJSONArray(names.getString(i)).let { a -> (0 until a.length()).map { a.getString(it).lowercase() } }
            val f = faces.getJSONArray(i).let { a -> (1 until a.length()).map(a::getString) }
            w to f
        }
    }

    /** The face for `sentence`, or null. `recent`: faces used, newest first. */
    fun suggest(ctx: Context, sentence: String, recent: List<String>): String? {
        if (sentence.isBlank()) return null
        load(ctx)
        val s = " " + sentence.lowercase() + " "
        var best: Pair<Int, List<String>>? = null
        for ((words, faces) in moods) {
            for (w in words) {
                val at = if (w.all { it.isLetter() || it == '\'' || it == ' ' }) Regex("(?<![\\p{L}])" + Regex.escape(w) + "(?![\\p{L}])").findAll(s).lastOrNull()?.range?.first ?: -1 else s.lastIndexOf(w)
                if (at >= 0 && (best == null || at > best.first)) best = at to faces
            }
        }
        val faces = best?.second ?: return null
        return recent.firstOrNull { it in faces } ?: faces.firstOrNull()
    }
}
