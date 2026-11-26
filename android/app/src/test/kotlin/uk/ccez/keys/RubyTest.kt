package uk.ccez.keys

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import uk.ccez.keys.ui.rubyStarts

/**
 * Where each reading sits over its word (ui/Ruby.kt): centred over its own
 * kanji, spreading past them when wider, never over the next reading.
 * gates/furigana-placement/goal.md; protected: the model may not edit it.
 */
class RubyTest {
    private fun near(want: List<Float>, got: List<Float>) {
        assertEquals("count", want.size, got.size)
        want.zip(got).forEachIndexed { i, (w, g) -> assertEquals("reading $i", w, g, 0.001f) }
    }

    @Test fun oneReadingIsCentredOverItsOwnBase() {
        near(listOf(10f), rubyStarts(listOf(40f), listOf(20f), gap = 2f))
    }

    @Test fun theOwnersWordEachRunGetsItsOwnReading() {
        // 思 い 浮 かべる (17 px a kanji), おも (19 px) and う (9.5 px); い and かべる get none.
        val got = rubyStarts(listOf(17f, 17f, 17f, 51f), listOf(19f, 0f, 9.5f, 0f), gap = 1f)
        near(listOf(-1f, 25.5f, 37.75f, 76.5f), got)
        // う sits over 浮 (34 to 51), not where the whole reading over the whole word would put it.
        assertTrue(got[2] >= 34f && got[2] + 9.5f <= 51f)
    }

    @Test fun wideReadingsSpreadButNeverOverlapTheNext() {
        val got = rubyStarts(listOf(10f, 10f, 10f), listOf(30f, 30f, 30f), gap = 2f)
        near(listOf(-10f, 22f, 54f), got)
        for (i in 1 until got.size) assertTrue("reading $i clears the one before", got[i] >= got[i - 1] + 30f + 2f - 0.001f)
    }

    @Test fun kanaWithoutReadingsNeitherPushNorArePushed() {
        near(listOf(-10f, 30f, 50f), rubyStarts(listOf(10f, 40f, 10f), listOf(30f, 0f, 10f), gap = 2f))
    }

    @Test fun aWordWithNoReadingsHasNothingToPlace() {
        near(listOf(15f, 45f), rubyStarts(listOf(30f, 30f), listOf(0f, 0f), gap = 2f))
    }

    @Test fun theWholeReadingOverTheWholeWordIsCentred() {
        // The fallback (今日, 物の怪): one part, the whole reading centred over it.
        near(listOf(15f), rubyStarts(listOf(100f), listOf(70f), gap = 1f))
        near(listOf(-20f), rubyStarts(listOf(60f), listOf(100f), gap = 1f))
    }

    @Test fun theListsMustMatch() {
        assertThrows(IllegalArgumentException::class.java) { rubyStarts(listOf(10f, 10f), listOf(5f), gap = 1f) }
    }
}
