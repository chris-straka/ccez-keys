package uk.ccez.keys

import org.junit.Assert.assertEquals
import org.junit.Test

class HangulTest {
    @Test
    fun syllablesJoin() {
        assertEquals("한글", Hangul.compose("ㅎㅏㄴㄱㅡㄹ"))
        assertEquals("안녕하세요", Hangul.compose("ㅇㅏㄴㄴㅕㅇㅎㅏㅅㅔㅇㅛ"))
    }

    @Test
    fun finalsMoveOnAndDoubleUp() {
        assertEquals("닭", Hangul.compose("ㄷㅏㄹㄱ"))
        assertEquals("달가", Hangul.compose("ㄷㅏㄹㄱㅏ"))
        assertEquals("값이", Hangul.compose("ㄱㅏㅂㅅㅇㅣ"))
    }

    @Test
    fun vowelsCombineAndStandAlone() {
        assertEquals("와", Hangul.compose("ㅇㅗㅏ"))
        assertEquals("의", Hangul.compose("ㅇㅡㅣ"))
        assertEquals("ㅏ", Hangul.compose("ㅏ"))
        assertEquals("ㄱ", Hangul.compose("ㄱ"))
    }
}
