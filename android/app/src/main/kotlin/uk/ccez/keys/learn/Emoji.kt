package uk.ccez.keys.learn

import android.icu.lang.UCharacter
import android.icu.lang.UProperty
import android.os.Build

/**
 * Emoji by name, for ":pizza" typed in a field (owner, 2026-11-15): the
 * phone's own Unicode names (ICU), so no table to keep. Each emoji shown
 * as an emoji by default; names are words ("slice of pizza").
 */
object Emoji {
    private var all: List<Pair<String, List<String>>>? = null

    private fun load(): List<Pair<String, List<String>>> {
        all?.let { return it }
        val out = ArrayList<Pair<String, List<String>>>()
        if (Build.VERSION.SDK_INT >= 28) {
            for (range in listOf(0x2300..0x23FF, 0x2600..0x27BF, 0x2B00..0x2BFF, 0x1F000..0x1FAFF)) {
                for (cp in range) {
                    if (!UCharacter.hasBinaryProperty(cp, UProperty.EMOJI_PRESENTATION)) continue
                    // Skin tones and regional letters aren't emoji alone.
                    if (UCharacter.hasBinaryProperty(cp, UProperty.EMOJI_MODIFIER) || cp in 0x1F1E6..0x1F1FF) continue
                    val name = UCharacter.getName(cp)?.lowercase() ?: continue
                    out += String(Character.toChars(cp)) to name.split(' ', '-').filter { it.isNotEmpty() }
                }
            }
        }
        all = out
        return out
    }

    /** Up to `n` emoji whose name has a word starting with `query` (2 letters or more), best first. */
    fun named(query: String, n: Int = 3): List<String> {
        val q = query.lowercase()
        if (q.length < 2 || !q.all { it.isLetter() }) return emptyList()
        return load()
            .mapNotNull { (e, words) ->
                val rank = when {
                    words.any { it == q } -> 0
                    words.any { it.startsWith(q) } -> 1
                    else -> return@mapNotNull null
                }
                Triple(e, rank, words.size)
            }
            .sortedWith(compareBy({ it.second }, { it.third }))
            .take(n)
            .map { it.first }
    }
}
