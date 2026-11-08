package uk.ccez.keys

/**
 * Korean 2-set typing: the letters (jamo) typed so far joined into
 * syllables, the standard way: a consonant after a vowel waits as the
 * final, and moves to the next syllable when a vowel follows it
 * (ㅎㅏㄴㄱㅡㄹ → 한글).
 */
object Hangul {
    private const val L = "ㄱㄲㄴㄷㄸㄹㅁㅂㅃㅅㅆㅇㅈㅉㅊㅋㅌㅍㅎ"
    private const val V = "ㅏㅐㅑㅒㅓㅔㅕㅖㅗㅘㅙㅚㅛㅜㅝㅞㅟㅠㅡㅢㅣ"
    private val T = listOf("", "ㄱ", "ㄲ", "ㄳ", "ㄴ", "ㄵ", "ㄶ", "ㄷ", "ㄹ", "ㄺ", "ㄻ", "ㄼ", "ㄽ", "ㄾ", "ㄿ", "ㅀ", "ㅁ", "ㅂ", "ㅄ", "ㅅ", "ㅆ", "ㅇ", "ㅈ", "ㅊ", "ㅋ", "ㅌ", "ㅍ", "ㅎ")
    private val VOWELS = mapOf("ㅗㅏ" to 'ㅘ', "ㅗㅐ" to 'ㅙ', "ㅗㅣ" to 'ㅚ', "ㅜㅓ" to 'ㅝ', "ㅜㅔ" to 'ㅞ', "ㅜㅣ" to 'ㅟ', "ㅡㅣ" to 'ㅢ')
    private val FINALS = mapOf(
        "ㄱㅅ" to 'ㄳ', "ㄴㅈ" to 'ㄵ', "ㄴㅎ" to 'ㄶ', "ㄹㄱ" to 'ㄺ', "ㄹㅁ" to 'ㄻ', "ㄹㅂ" to 'ㄼ',
        "ㄹㅅ" to 'ㄽ', "ㄹㅌ" to 'ㄾ', "ㄹㅍ" to 'ㄿ', "ㄹㅎ" to 'ㅀ', "ㅂㅅ" to 'ㅄ',
    )
    private val SPLIT = FINALS.entries.associate { (k, v) -> v to (k[0] to k[1]) }

    fun isJamo(c: Char) = c in 'ㄱ'..'ㆎ'
    private fun isVowel(c: Char) = c in V

    fun compose(jamo: CharSequence): String {
        val out = StringBuilder()
        var l: Char? = null
        var v: Char? = null
        var t: Char? = null
        fun flush() {
            when {
                l != null && v != null -> out.append((0xAC00 + (L.indexOf(l!!) * 21 + V.indexOf(v!!)) * 28 + T.indexOf(t?.toString() ?: "")).toChar())
                l != null -> out.append(l)
                v != null -> out.append(v)
            }
            if (l == null && v == null && t != null) out.append(t)
            l = null; v = null; t = null
        }
        for (c in jamo) {
            if (!isVowel(c)) {
                when {
                    l == null && v == null -> l = c
                    v == null -> { flush(); l = c }
                    l == null -> { flush(); l = c }
                    t == null -> if (T.contains(c.toString())) t = c else { flush(); l = c }
                    else -> {
                        val both = FINALS["$t$c"]
                        if (both != null) t = both else { flush(); l = c }
                    }
                }
            } else {
                when {
                    v == null && t == null -> v = c
                    t == null -> {
                        val both = VOWELS["$v$c"]
                        if (both != null) v = both else { flush(); v = c }
                    }
                    else -> {
                        // The final starts the next syllable (a double one gives up its second half).
                        val split = SPLIT[t!!]
                        val next: Char
                        if (split != null) { t = split.first; next = split.second } else { next = t!!; t = null }
                        flush()
                        l = next
                        v = c
                    }
                }
            }
        }
        flush()
        return out.toString()
    }
}
