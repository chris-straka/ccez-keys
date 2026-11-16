package uk.ccez.keys.learn

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject

/**
 * The phone's clipboard from the Mac: `keys clip put|now|ls|get|rm|clear`,
 * through Control (adb only; no other app can reach it). Works with the
 * keyboard closed: Android lets any app write the clipboard, and lets the
 * chosen keyboard read it, without focus.
 *
 * Ids are the text's Java hash as an unsigned number: the same text has
 * the same id on every call, so `ls` then `rm <id>` needs no state.
 */
object ClipBoard {
    /**
     * The text an agent last put and the clip it replaced (memory only):
     * `rm` of it puts that back. `known` is false when the phone was
     * locked at the put, so what it replaced couldn't be read.
     */
    private class Replaced(val text: String, val before: ClipData?, val known: Boolean)
    private var replaced: Replaced? = null

    private fun locked(ctx: Context) = ctx.getSystemService(android.app.KeyguardManager::class.java)?.isDeviceLocked == true

    fun id(text: String): Long = text.hashCode().toLong() and 0xffffffffL

    fun run(ctx: Context, arg: String, value: String?, clearChip: () -> Unit): String {
        val cm = ctx.getSystemService(ClipboardManager::class.java) ?: error("no clipboard on this phone")
        val parts = arg.trim().split(' ').filter { it.isNotEmpty() }
        return when (parts.firstOrNull()) {
            "put" -> {
                val text = value ?: error("clip put needs text")
                if (text.isEmpty()) error("nothing to put")
                val wasLocked = locked(ctx)
                val before = if (wasLocked) null else runCatching { cm.primaryClip }.getOrNull()
                cm.setPrimaryClip(ClipData.newPlainText("ccez-keys", text))
                replaced = Replaced(text.trim().take(10_000), before, !wasLocked)
                // The keyboard's listener adds it too while it runs; added here so it's in `ls` at once.
                if (!uk.ccez.keys.core.looksSecret(text)) Clips.add(ctx, text.trim().take(10_000))
                val out = JSONObject().put("id", id(text.trim().take(10_000)))
                if (wasLocked) {
                    out.put("on_clipboard", JSONObject.NULL).put("locked", true)
                } else {
                    if (current(cm, ctx) != text) error("the clipboard didn't take it")
                    out.put("on_clipboard", true)
                }
                out.toString(2)
            }
            "now" -> if (locked(ctx)) error("the phone is locked: Android hides the clipboard until it's unlocked") else now(cm, ctx).toString(2)
            "ls" -> {
                val isLocked = locked(ctx)
                val on = if (isLocked) null else current(cm, ctx)
                val pins = Clips.pinned(ctx)
                val rows = (pins + Clips.all(ctx).filter { it !in pins }).map { t ->
                    JSONObject().put("id", id(t)).put("text", t).put("pinned", t in pins).put("on_clipboard", if (isLocked) JSONObject.NULL else on != null && on.trim() == t)
                }
                JSONArray(rows).toString(2)
            }
            "get" -> find(ctx, parts.getOrNull(1)) ?: error("no item ${parts.getOrNull(1)}")
            "rm" -> {
                val want = parts.getOrNull(1) ?: error("clip rm needs an id")
                val text = find(ctx, want) ?: error("no item $want")
                Clips.remove(ctx, text)
                val r = replaced?.takeIf { it.text == text }
                // Locked, the clipboard can't be read: an agent's own put is taken to be on it still.
                val on = if (locked(ctx)) r != null else current(cm, ctx)?.trim() == text
                val clipboard = if (!on) "untouched" else {
                    // An agent's put taken back: what was there before comes back.
                    val back = r?.before
                    replaced = null
                    if (back != null && runCatching { cm.setPrimaryClip(back) }.isSuccess) "put back what was there before" else {
                        clear(cm)
                        clearChip()
                        "cleared"
                    }
                }
                JSONObject().put("removed", id(text)).put("clipboard", clipboard).toString(2)
            }
            "clear" -> {
                clear(cm)
                replaced = null
                clearChip()
                val left = runCatching { cm.primaryClip }.getOrNull()
                if (left != null && left.itemCount > 0 && !left.getItemAt(0).coerceToText(ctx).isNullOrEmpty()) error("the clipboard wasn't cleared")
                JSONObject().put("cleared", true).toString(2)
            }
            else -> error("clip put | now | ls | get <id> | rm <id> | clear")
        }
    }

    private fun find(ctx: Context, want: String?): String? {
        val n = want?.toLongOrNull() ?: return null
        return (Clips.pinned(ctx) + Clips.all(ctx)).firstOrNull { id(it) == n }
    }

    private fun clear(cm: ClipboardManager) {
        if (Build.VERSION.SDK_INT >= 28) cm.clearPrimaryClip() else cm.setPrimaryClip(ClipData.newPlainText("", ""))
    }

    private fun sensitive(clip: ClipData) =
        Build.VERSION.SDK_INT >= 33 && clip.description.extras?.getBoolean(ClipDescription.EXTRA_IS_SENSITIVE) == true

    /** The clipboard's text now; null when empty, a picture or marked sensitive. */
    private fun current(cm: ClipboardManager, ctx: Context): String? {
        val clip = runCatching { cm.primaryClip }.getOrNull() ?: return null
        if (clip.itemCount == 0 || sensitive(clip)) return null
        return clip.getItemAt(0).coerceToText(ctx)?.toString()?.ifEmpty { null }
    }

    /** What's on the clipboard, never a password: an app's sensitive mark or a key-looking text hides it. */
    private fun now(cm: ClipboardManager, ctx: Context): JSONObject {
        val clip = runCatching { cm.primaryClip }.getOrNull()
        if (clip == null || clip.itemCount == 0) return JSONObject().put("empty", true)
        if (sensitive(clip)) return JSONObject().put("secret", true)
        val item = clip.getItemAt(0)
        if (clip.description.hasMimeType("image/*") || (item.uri != null && item.text == null)) return JSONObject().put("picture", true)
        val text = item.coerceToText(ctx)?.toString().orEmpty()
        if (text.isEmpty()) return JSONObject().put("empty", true)
        if (uk.ccez.keys.core.looksSecret(text)) return JSONObject().put("secret", true)
        return JSONObject().put("id", id(text.trim())).put("text", text)
    }
}
