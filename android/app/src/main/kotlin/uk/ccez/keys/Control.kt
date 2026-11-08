package uk.ccez.keys

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.Binder
import android.os.Bundle
import android.os.Process
import android.os.Build
import android.view.inputmethod.InputMethodInfo
import android.view.inputmethod.InputMethodManager
import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import uk.ccez.keys.core.KeysException

/**
 * What `keys` on the Mac calls, through `adb shell content call --uri
 * content://uk.ccez.keys.control --method <status|get|set|schema|log>`.
 * Answers one extra, "out" (or "error"), base64 so any text survives the
 * shell. Only the shell user (adb) and this app may call it.
 */
class Control : ContentProvider() {
    override fun onCreate() = true

    override fun call(method: String, arg: String?, extras: Bundle?): Bundle {
        val uid = Binder.getCallingUid()
        if (uid != Process.SHELL_UID && uid != Process.ROOT_UID && uid != Process.myUid()) {
            throw SecurityException("only adb may control ccez-keys")
        }
        val app = App.instance
        val engine = app.engine
        val out = Bundle()
        try {
            val result = when (method) {
                "status" -> status(app)
                "get" -> if (arg == null) engine.settingsJson() else engine.setting(arg)
                "set" -> {
                    val key = arg ?: error("set needs a setting name")
                    val value = extras?.getString("value")?.let { String(Base64.decode(it, Base64.DEFAULT)) } ?: error("set needs a value")
                    engine.setSetting(key, value).also { app.settingsChanged() }
                }
                "schema" -> engine.settingsSchema()
                // A word's status: `keys word fr libre` reads it, `keys word fr libre new|learning|known` sets it (and syncs).
                "word" -> {
                    val parts = (arg ?: error("word needs: lang word [status]")).split(' ')
                    if (parts.size < 2) error("word needs: lang word [status]")
                    val (lang, w) = parts
                    if (parts.size > 2) {
                        val st = when (parts[2]) { "new" -> uk.ccez.keys.core.WordStatus.NEW; "learning" -> uk.ccez.keys.core.WordStatus.LEARNING; "known" -> uk.ccez.keys.core.WordStatus.KNOWN; else -> error("status is new, learning or known") }
                        engine.setWordStatus(lang, w, st)
                        uk.ccez.keys.learn.WordSync.soon(500)
                    }
                    engine.wordStatus(lang, w).name.lowercase()
                }
                // Look for an update now.
                "update" -> {
                    app.getSharedPreferences("updates", android.content.Context.MODE_PRIVATE).edit().putLong("checked", 0).putLong("pinged", 0).apply()
                    if (engine.setting("updates.server").trim('"').isNotEmpty()) { Updater.closed(app); "asked the update service" } else { Updater.maybe(app); "checking" }
                }
                // Corrections the owner took, as JSON lines (their own writing: for ccez-learn).
                "mistakes" -> engine.mistakes()
                // The language typed in, as if picked from the space bar.
                "use" -> {
                    val lang = arg ?: error("use needs a language code")
                    if (lang !in engine.typingLanguages()) error("$lang isn't one of the languages typed in: ${engine.typingLanguages().joinToString(" ")}")
                    engine.setCurrentLanguage(lang)
                    app.settingsChanged()
                    lang
                }
                // The AI key: from `keys key` (stdin on the Mac, read on the phone's shell).
                "key" -> {
                    val value = extras?.getString("value")?.let { String(Base64.decode(it, Base64.DEFAULT)) } ?: error("key needs a value")
                    uk.ccez.keys.learn.Secrets.set(app, value.trim().ifEmpty { null })
                    app.log("info", "ai-key", "set" to value.isNotBlank())
                    if (value.isBlank()) "removed" else "saved"
                }
                "log" -> engine.logTail((arg?.toIntOrNull() ?: 50).coerceIn(1, 5000).toUInt())
                else -> error("no command called $method")
            }
            out.putString("out", b64(result))
        } catch (e: KeysException.Bad) {
            out.putString("error", b64(e.v1))
        } catch (e: Exception) {
            app.log("error", "control", "method" to method, "error" to e.toString())
            out.putString("error", b64(e.message ?: e.toString()))
        }
        return out
    }

    private fun status(app: App): String {
        val imm = app.getSystemService(InputMethodManager::class.java)
        val ours = { info: InputMethodInfo -> info.packageName == app.packageName }
        val e = app.engine
        val o = JSONObject()
            .put("version", BuildConfig.VERSION_NAME)
            .put("core", e.version())
            .put("enabled", imm?.enabledInputMethodList?.any(ours) == true)
        if (Build.VERSION.SDK_INT >= 34) o.put("chosen", imm?.currentInputMethodInfo?.let(ours) == true)
        return o
            .put("typing", JSONArray(e.typingLanguages()))
            .put("current", e.currentLanguage())
            .put("ai_key", uk.ccez.keys.learn.Secrets.get(app) != null)
            .put("words_loaded", JSONArray(e.typingLanguages().filter { e.loaded(it) }))
            .toString(2)
    }

    private fun b64(s: String) = Base64.encodeToString(s.toByteArray(), Base64.NO_WRAP)

    override fun query(uri: Uri, p: Array<out String>?, s: String?, a: Array<out String>?, o: String?): Cursor? = null
    override fun getType(uri: Uri): String? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, s: String?, a: Array<out String>?) = 0
    override fun update(uri: Uri, v: ContentValues?, s: String?, a: Array<out String>?) = 0
}
