package uk.ccez.keys

import org.junit.Assert.assertEquals
import org.junit.Test
import uk.ccez.keys.learn.Clips
import uk.ccez.keys.learn.Latest

class ClipsOrderTest {
    @Test
    fun copiesAndPicturesTogetherNewestFirst() {
        // Pictures stand in as timed items (a Bitmap needs a device).
        val texts = listOf(Latest.Text("farouche", 300), Latest.Text("lugubre", 100), Latest.Text("d'avant", 0))
        val out = Clips.order(texts, listOf(Latest.Text("picture A", 200), Latest.Text("screenshot", 400)))
        assertEquals(listOf("screenshot", "farouche", "picture A", "lugubre", "d'avant"), out.map { (it as Latest.Text).text })
    }
}
