package uk.ccez.keys

import org.junit.Assert.assertEquals
import org.junit.Test
import uk.ccez.keys.core.Key
import uk.ccez.keys.core.KeyKind

class LabelTest {
    private fun key(label: String, alt: String = "") = Key(KeyKind.CHAR, label, label, alt, "", emptyList(), 1f)

    @Test
    fun lowercaseUnlessTheNextLetterIsACapital() {
        assertEquals("é", charLabel(key("é"), Shift.OFF, true))
        assertEquals("É", charLabel(key("é"), Shift.ONCE, true))
        assertEquals("Ж", charLabel(key("ж"), Shift.LOCK, true))
        assertEquals(listOf("À", "Â", "Æ"), listOf("à", "â", "æ").map { shown(it, Shift.ONCE, true) })
    }

    @Test
    fun symbolsKeepTheirCase() {
        assertEquals("π", charLabel(key("π"), Shift.ONCE, false))
        assertEquals("ß", shown("ß", Shift.ONCE, false))
    }

    @Test
    fun scriptsWithoutCaseShowTheirShiftedLetters() {
        assertEquals("ก", charLabel(key("ก", alt = "ฆ"), Shift.OFF, true))
        assertEquals("ฆ", charLabel(key("ก", alt = "ฆ"), Shift.ONCE, true))
        assertEquals("ㅃ", charLabel(key("ㅂ", alt = "ㅃ"), Shift.ONCE, true))
    }
}
