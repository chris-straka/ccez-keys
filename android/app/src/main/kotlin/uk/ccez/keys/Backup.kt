package uk.ccez.keys

import android.content.Context
import org.json.JSONObject
import java.io.File

/**
 * The keyboard's own state as one JSON, for `keys backup` / `keys restore`
 * on the Mac (a new phone set up in one step): settings, the language last
 * used, the words learned from typing (per language), the language per app,
 * kaomoji use and the review schedule. Not the clipboard (it stays on the
 * phone, owner's call) and not the AI key (the phone's keystore).
 */
object Backup {
    /** Writing failed partway: some of the backup is in place. */
    class WriteFailed(cause: Throwable) : Exception("restore stopped partway: $cause", cause)

    private val PREFS = listOf("app-languages", "faces", "widget-later")
    /** Files in the core's folder that are the owner's (dictionaries and logs aren't). */
    private val FILE = Regex("""^(settings\.json|screen-words\.json|current-language|user-[a-z]{2,3}(-[A-Za-z]{2,8})?\.json)$""")

    private fun dir(ctx: Context) = File(ctx.filesDir, "keys")

    fun export(ctx: Context): String {
        val files = JSONObject()
        dir(ctx).listFiles()?.filter { FILE.matches(it.name) }?.sortedBy { it.name }?.forEach { files.put(it.name, it.readText()) }
        val prefs = JSONObject()
        for (name in PREFS) {
            val p = JSONObject()
            for ((k, v) in ctx.getSharedPreferences(name, Context.MODE_PRIVATE).all) {
                when (v) {
                    is String -> p.put(k, JSONObject().put("s", v))
                    is Boolean -> p.put(k, JSONObject().put("b", v))
                    is Int -> p.put(k, JSONObject().put("i", v))
                    is Long -> p.put(k, JSONObject().put("l", v))
                    is Float -> p.put(k, JSONObject().put("f", v.toDouble()))
                }
            }
            prefs.put(name, p)
        }
        return JSONObject()
            .put("ccez-keys-backup", 1)
            .put("version", BuildConfig.VERSION_NAME)
            .put("made", System.currentTimeMillis())
            .put("files", files)
            .put("prefs", prefs)
            .toString()
    }

    /** Puts a backup back (what it holds replaces what's here); the keyboard then starts afresh to read it. */
    fun import(ctx: Context, json: String): String {
        // All of it read and checked first: a bad backup changes nothing.
        val o = JSONObject(json)
        require(o.optInt("ccez-keys-backup") == 1) { "not a ccez-keys backup" }
        val files = o.getJSONObject("files")
        val write = files.keys().asSequence().filter { FILE.matches(it) }.associateWith { name ->
            // Text, really (org.json would turn a number into one), and a .json file whole JSON.
            val v = files.get(name) as? String ?: error("$name isn't text")
            if (name.endsWith(".json")) require(runCatching { org.json.JSONTokener(v).nextValue() is JSONObject }.getOrDefault(false)) { "$name isn't a JSON object" }
            v
        }
        require(write.isNotEmpty()) { "nothing to restore" }
        val prefs = o.optJSONObject("prefs") ?: JSONObject()
        val groups = PREFS.mapNotNull { name ->
            val p = prefs.optJSONObject(name) ?: return@mapNotNull null
            name to p.keys().asSequence().map { k ->
                val v = p.getJSONObject(k)
                k to when {
                    v.has("s") -> v.get("s") as? String ?: error("$name.$k isn't text")
                    v.has("b") -> v.get("b") as? Boolean ?: error("$name.$k isn't true or false")
                    v.has("i") -> (v.get("i") as? Number)?.toInt() ?: error("$name.$k isn't a number")
                    v.has("l") -> (v.get("l") as? Number)?.toLong() ?: error("$name.$k isn't a number")
                    v.has("f") -> (v.get("f") as? Number)?.toFloat() ?: error("$name.$k isn't a number")
                    else -> error("$name.$k has no value")
                }
            }.toList()
        }
        // Then written, each file whole (written aside, moved into place).
        try {
            write(ctx, write, groups)
        } catch (e: Exception) {
            throw WriteFailed(e)
        }
        return "restored ${write.size} files and ${groups.size} settings groups from ${o.optString("version")}"
    }

    private fun write(ctx: Context, write: Map<String, String>, groups: List<Pair<String, List<Pair<String, Any>>>>) {
        val d = dir(ctx).also { it.mkdirs() }
        for ((name, text) in write) {
            val tmp = File(d, "$name.restore")
            tmp.writeText(text)
            if (!tmp.renameTo(File(d, name))) error("couldn't write $name")
        }
        for ((name, entries) in groups) {
            val e = ctx.getSharedPreferences(name, Context.MODE_PRIVATE).edit().clear()
            for ((k, v) in entries) {
                when (v) {
                    is String -> e.putString(k, v)
                    is Boolean -> e.putBoolean(k, v)
                    is Int -> e.putInt(k, v)
                    is Long -> e.putLong(k, v)
                    is Float -> e.putFloat(k, v)
                }
            }
            e.commit()
        }
    }
}
