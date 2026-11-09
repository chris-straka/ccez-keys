package uk.ccez.keys

import android.text.TextUtils
import android.view.KeyEvent
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputConnectionWrapper

/**
 * The app's text connection, remembering the text before the cursor until
 * something changes it: each read is a call into the app, and one key used
 * to make up to four (the sentence, the word before, capitals). Edits made
 * through it, and `forget()` (the cursor moved), drop what it remembers.
 */
class CachedInput(target: InputConnection) : InputConnectionWrapper(target, false) {
    /** The connection it wraps (to know when the field changes). */
    var base: InputConnection? = null
    private var before: String? = null
    /** It holds everything before the cursor (the field's start is in it). */
    private var whole = false

    fun forget() { before = null }

    override fun getTextBeforeCursor(n: Int, flags: Int): CharSequence? {
        if (flags != 0) return super.getTextBeforeCursor(n, flags)
        val b = before
        if (b != null && (b.length >= n || whole)) return b.takeLast(n)
        val want = maxOf(n, READ)
        val got = super.getTextBeforeCursor(want, 0)?.toString() ?: return null
        before = got
        whole = got.length < want
        return got.takeLast(n)
    }

    override fun getCursorCapsMode(reqModes: Int): Int {
        val t = getTextBeforeCursor(READ, 0) ?: return super.getCursorCapsMode(reqModes)
        return TextUtils.getCapsMode(t, t.length, reqModes)
    }

    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean { forget(); return super.commitText(text, newCursorPosition) }
    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean { forget(); return super.setComposingText(text, newCursorPosition) }
    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean { forget(); return super.deleteSurroundingText(beforeLength, afterLength) }
    override fun deleteSurroundingTextInCodePoints(beforeLength: Int, afterLength: Int): Boolean { forget(); return super.deleteSurroundingTextInCodePoints(beforeLength, afterLength) }
    override fun finishComposingText(): Boolean { forget(); return super.finishComposingText() }
    override fun setComposingRegion(start: Int, end: Int): Boolean { forget(); return super.setComposingRegion(start, end) }
    override fun setSelection(start: Int, end: Int): Boolean { forget(); return super.setSelection(start, end) }
    override fun sendKeyEvent(event: KeyEvent?): Boolean { forget(); return super.sendKeyEvent(event) }
    override fun performEditorAction(editorAction: Int): Boolean { forget(); return super.performEditorAction(editorAction) }
    override fun commitContent(inputContentInfo: android.view.inputmethod.InputContentInfo, flags: Int, opts: android.os.Bundle?): Boolean { forget(); return super.commitContent(inputContentInfo, flags, opts) }

    companion object {
        /** Enough for the sentence (300) and the word before. */
        const val READ = 320
    }
}
