package uk.ccez.keys

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uk.ccez.keys.learn.KeyNames

/** Named keys (`keys key add <name>`): which names, and where each lives. */
class KeyNamesTest {
    @Test fun deepseekStaysWhereTheOneKeyAlwaysWas() {
        assertEquals("ai", KeyNames.pref("deepseek"))
        assertEquals("key:gemini", KeyNames.pref("gemini"))
    }

    @Test fun namesComeFromThePrefsDeepseekFirst() {
        assertEquals(listOf("deepseek", "gemini", "mistral"), KeyNames.names(listOf("version", "key:mistral", "at:gemini", "ai", "key:gemini", "key:Bad Name")))
        assertEquals(listOf("gemini"), KeyNames.names(listOf("version", "key:gemini")))
        assertEquals(emptyList<String>(), KeyNames.names(listOf("version")))
    }

    @Test fun onlyPlainNames() {
        assertTrue(KeyNames.valid("deepseek"))
        assertTrue(KeyNames.valid("open-router2"))
        assertFalse(KeyNames.valid("DeepSeek"))
        assertFalse(KeyNames.valid("a b"))
        assertFalse(KeyNames.valid("-x"))
        assertFalse(KeyNames.valid(""))
        assertFalse(KeyNames.valid("a".repeat(33)))
    }

    @Test fun settingsLabels() {
        assertEquals("DeepSeek", KeyNames.label("deepseek"))
        assertEquals("Gemini", KeyNames.label("gemini"))
        assertEquals("Mistral", KeyNames.label("mistral"))
    }
}
