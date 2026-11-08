package uk.ccez.keys.learn

import android.content.Context
import org.json.JSONArray
import java.io.File

/**
 * The last 30 things copied (text), newest first, kept in the app's own
 * storage. Nothing an app marked sensitive is ever added (see KeysService).
 */
object Clips {
    private const val MAX = 30
    private var items: MutableList<String>? = null

    private fun file(ctx: Context) = File(ctx.filesDir, "clips.json")

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
    }

    fun remove(ctx: Context, text: String) = save(ctx, all(ctx) - text)

    private fun save(ctx: Context, l: List<String>) {
        items = l.toMutableList()
        runCatching { file(ctx).writeText(JSONArray(l).toString()) }
    }
}
