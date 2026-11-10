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
    private val PREFS = listOf("app-languages", "faces", "widget-later")
    /** Files in the core's folder that are the owner's (dictionaries and logs aren't). */
    private val FILE = Regex("""^(settings\.json|current-language|user-[a-z]{2,3}(-[A-Za-z]{2,8})?\.json)$""")

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
        val o = JSONObject(json)
        require(o.optInt("ccez-keys-backup") == 1) { "not a ccez-keys backup" }
        val files = o.getJSONObject("files")
        val d = dir(ctx).also { it.mkdirs() }
        var n = 0
        for (name in files.keys()) {
            // Only the owner's own files, by name: nothing else can be written.
            if (!FILE.matches(name)) continue
            val tmp = File(d, "$name.restore")
            tmp.writeText(files.getString(name))
            if (!tmp.renameTo(File(d, name))) error("couldn't write $name")
            n++
        }
        val prefs = o.optJSONObject("prefs") ?: JSONObject()
        for (name in PREFS) {
            val p = prefs.optJSONObject(name) ?: continue
            val e = ctx.getSharedPreferences(name, Context.MODE_PRIVATE).edit().clear()
            for (k in p.keys()) {
                val v = p.getJSONObject(k)
                when {
                    v.has("s") -> e.putString(k, v.getString("s"))
                    v.has("b") -> e.putBoolean(k, v.getBoolean("b"))
                    v.has("i") -> e.putInt(k, v.getInt("i"))
                    v.has("l") -> e.putLong(k, v.getLong("l"))
                    v.has("f") -> e.putFloat(k, v.getDouble("f").toFloat())
                }
            }
            e.commit()
        }
        return "restored $n files and ${prefs.length()} settings groups from ${o.optString("version")}"
    }
}
