package uk.ccez.keys

import org.junit.Assert.assertEquals
import org.junit.Test
import uk.ccez.keys.learn.articleFrom
import uk.ccez.keys.learn.bare
import uk.ccez.keys.learn.inContext

class StripTest {
    @Test fun endPunctuationGoes() {
        assertEquals("テスト", bare("テスト・"))
        assertEquals("chat", bare("chat."))
        assertEquals("¿qué", bare("¿qué?"))
        assertEquals("l'eau", bare("l'eau"))
    }

    @Test fun meaningInTheSentenceComesFirst() {
        val livre = listOf("book", "(Europe, informal) metrical pound, half a kilogramme", "third-person singular of livrer (to deliver)")
        assertEquals("book", inContext(livre, "I read a book")[0])
        assertEquals(livre[1], inContext(livre, "a pound of butter")[0])
        assertEquals(livre[2], inContext(livre, "they deliver it today")[0])
        // Nothing matching: the dictionary's order.
        assertEquals(livre, inContext(livre, "hello"))
    }

    @Test fun articleFromTheSentence() {
        assertEquals("la", articleFrom("fr", "une", "livre"))
        assertEquals("le", articleFrom("fr", "un", "livre"))
        assertEquals("l'", articleFrom("fr", "une", "eau"))
        assertEquals("el", articleFrom("es", "un", "libro"))
        assertEquals("a", articleFrom("pt", "uma", "casa"))
        assertEquals(null, articleFrom("fr", "les", "livres"))
        assertEquals(null, articleFrom("de", "ein", "Buch"))
    }
}
