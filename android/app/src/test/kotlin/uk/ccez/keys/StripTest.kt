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

    @Test fun reviewArticles() {
        assertEquals("", uk.ccez.keys.learn.Review.article("l'ardoise", "une"))
        assertEquals("", uk.ccez.keys.learn.Review.article("la gare", "la"))
        assertEquals("la", uk.ccez.keys.learn.Review.article("gare", "la"))
        assertEquals("", uk.ccez.keys.learn.Review.article("Bahnhof", ""))
        assertEquals("el", uk.ccez.keys.learn.Review.article("libro", "el"))
    }

    @org.junit.Test
    fun aNewMessageStartingWithTheSameLetterIsANewSentence() {
        // The owner's case: the strip kept its language from "I" into "Idk how it works".
        org.junit.Assert.assertFalse(uk.ccez.keys.learn.sameSentenceAs("I", "Idk how it works"))
        org.junit.Assert.assertFalse(uk.ccez.keys.learn.sameSentenceAs("Really", "Ready to go"))
        // Typed on, or a little taken back: the same.
        org.junit.Assert.assertTrue(uk.ccez.keys.learn.sameSentenceAs("Je pen", "Je pense que oui"))
        org.junit.Assert.assertTrue(uk.ccez.keys.learn.sameSentenceAs("Je pense que oui", "Je pense"))
        org.junit.Assert.assertTrue(uk.ccez.keys.learn.sameSentenceAs("Je", "Je pense"))
        org.junit.Assert.assertFalse(uk.ccez.keys.learn.sameSentenceAs("", "Je"))
    }
}
