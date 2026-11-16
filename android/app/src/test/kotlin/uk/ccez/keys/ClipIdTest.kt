package uk.ccez.keys

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uk.ccez.keys.learn.ClipBoard

class ClipIdTest {
    @Test
    fun sameTextSameIdNeverNegative() {
        // `keys clip ls` then `keys clip rm <id>` in a later call: the id comes from the text alone.
        assertEquals(ClipBoard.id("à tout à l'heure"), ClipBoard.id("à tout à l'heure"))
        assertNotEquals(ClipBoard.id("déjà"), ClipBoard.id("deja"))
        for (t in listOf("déjà", "zzzzzzzzzzzzzzzz", "今日は", "Übermorgen")) assertTrue(ClipBoard.id(t) >= 0)
    }
}
