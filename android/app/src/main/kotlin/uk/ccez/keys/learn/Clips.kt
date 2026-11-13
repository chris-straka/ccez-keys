package uk.ccez.keys.learn

import android.content.Context
import org.json.JSONArray
import java.io.File

/**
 * The last 30 things copied (text), newest first, kept in the app's own
 * storage, and the ones pinned (kept until unpinned, never pushed out).
 * Nothing an app marked sensitive is ever added (see KeysService).
 */
object Clips {
    private const val MAX = 30
    private var items: MutableList<String>? = null
    private var pins: MutableList<String>? = null

    private fun file(ctx: Context) = File(ctx.filesDir, "clips.json")
    private fun pinFile(ctx: Context) = File(ctx.filesDir, "clips-pinned.json")

    fun pinned(ctx: Context): List<String> {
        pins?.let { return it }
        val l = runCatching { JSONArray(pinFile(ctx).readText()).let { a -> (0 until a.length()).map(a::getString) } }.getOrDefault(emptyList())
        return l.toMutableList().also { pins = it }
    }

    fun pin(ctx: Context, text: String, on: Boolean) {
        val l = pinned(ctx).toMutableList()
        l.remove(text)
        if (on) l.add(0, text)
        pins = l
        runCatching { pinFile(ctx).writeText(JSONArray(l).toString()) }
    }

    fun all(ctx: Context): List<String> {
        items?.let { return it }
        val l = runCatching { JSONArray(file(ctx).readText()).let { a -> (0 until a.length()).map(a::getString) } }.getOrDefault(emptyList())
        return l.toMutableList().also { items = it }
    }

    fun add(ctx: Context, text: String) {
        val l = all(ctx).toMutableList()
        l.remove(text)
        l.add(0, text)
        save(ctx, l.take(MAX))
        val now = System.currentTimeMillis()
        prefs(ctx).edit().putLong("text-at", now).apply()
        // When each was copied, to put them in order with pictures and screenshots.
        val at = times(ctx)
        at.put(text, now)
        at.keys().asSequence().toList().filter { it !in l.take(MAX) }.forEach(at::remove)
        runCatching { timeFile(ctx).writeText(at.toString()) }
    }

    private fun timeFile(ctx: Context) = File(ctx.filesDir, "clips-at.json")
    private fun times(ctx: Context): org.json.JSONObject = runCatching { org.json.JSONObject(timeFile(ctx).readText()) }.getOrDefault(org.json.JSONObject())

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("clips", Context.MODE_PRIVATE)

    /** A picture copied (its address, while the app that copied it lets it be read). */
    fun pictureCopied(ctx: Context, uri: android.net.Uri) {
        prefs(ctx).edit().putString("picture", uri.toString()).putLong("picture-at", System.currentTimeMillis()).apply()
    }

    /**
     * The newest [n] things copied or screenshot, newest first (owner,
     * 2026-11-12: several in the bar, each a tap away, newest on the left):
     * the texts copied, the last picture copied, the last screenshot.
     * Reads storage: off the main thread.
     */
    fun recent(ctx: Context, n: Int): List<Latest> {
        val p = prefs(ctx)
        val at = times(ctx)
        val texts = all(ctx).take(n).mapIndexed { i, t -> Latest.Text(t, at.optLong(t, if (i == 0) p.getLong("text-at", 0) else 0)) }
        val copied = p.getString("picture", null)?.let { u ->
            val uri = android.net.Uri.parse(u)
            Shots.thumb(ctx, uri)?.let { Latest.Picture(uri, it, p.getLong("picture-at", 0)) }
        }
        val shot = Shots.newest(ctx)?.let { (uri, at) -> Shots.thumb(ctx, uri)?.let { Latest.Picture(uri, it, at) } }
        return order(texts, listOfNotNull(copied, shot).distinctBy { it.uri }).take(n)
    }

    /**
     * Texts (newest first, some with no time, from before times were
     * kept) and pictures, together newest first: a picture goes before
     * the first text older than it; texts keep their own order.
     */
    internal fun order(texts: List<Latest.Text>, pictures: List<Latest>): List<Latest> {
        val out = mutableListOf<Latest>()
        val pics = pictures.sortedByDescending { it.at }.toMutableList()
        for (t in texts) {
            while (pics.isNotEmpty() && pics.first().at > t.at) out += pics.removeAt(0)
            out += t
        }
        return out + pics
    }

    fun remove(ctx: Context, text: String) {
        save(ctx, all(ctx) - text)
        if (text in pinned(ctx)) pin(ctx, text, false)
    }

    private fun save(ctx: Context, l: List<String>) {
        items = l.toMutableList()
        runCatching { file(ctx).writeText(JSONArray(l).toString()) }
    }
}

/** The newest thing copied or screenshot. */
sealed interface Latest {
    /** When, in ms since 1970. */
    val at: Long
    data class Text(val text: String, override val at: Long) : Latest
    data class Picture(val uri: android.net.Uri, val thumb: android.graphics.Bitmap, override val at: Long) : Latest
}
