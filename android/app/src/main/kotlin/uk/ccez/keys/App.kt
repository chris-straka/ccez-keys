package uk.ccez.keys

import android.app.Application
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import org.json.JSONObject
import uk.ccez.keys.core.Engine

/** The one Engine (Rust core) for the keyboard, the settings screen and `keys`. */
class App : Application() {
    lateinit var engine: Engine
        private set

    /** Bumped whenever a setting changes (from the panel or `keys`). */
    val settingsVersion = MutableStateFlow(0)

    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onCreate() {
        super.onCreate()
        instance = this
        engine = Engine(filesDir.resolve("keys").path)
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            runCatching { log("error", "crash", "thread" to t.name, "error" to e.toString(), "at" to e.stackTrace.take(8).joinToString(" < ")) }
            previous?.uncaughtException(t, e)
        }
        log("info", "start", "version" to BuildConfig.VERSION_NAME)
        loadLanguages()
    }

    /** Loads every typing language's words, off the main thread. */
    fun loadLanguages() = scope.launch { engine.typingLanguages().forEach { ensureWords(it) } }

    /** A language's word list, once. Safe from any thread. */
    @Synchronized
    fun ensureWords(lang: String) {
        if (engine.loaded(lang)) return
        val started = System.nanoTime()
        val text = runCatching { assets.open("words/$lang.txt").bufferedReader().readText() }.getOrNull()
        if (text == null) {
            log("warn", "no-words", "lang" to lang)
            return
        }
        val extra = runCatching { assets.open("words/$lang-extra.txt").bufferedReader().readText() }.getOrDefault("")
        val n = engine.loadWords(lang, text + "\n" + extra)
        log("info", "words", "lang" to lang, "count" to n.toLong(), "ms" to (System.nanoTime() - started) / 1_000_000)
    }

    fun settingsChanged() {
        settingsVersion.value += 1
        loadLanguages()
    }

    /** Never pass typed text here. */
    fun log(level: String, event: String, vararg ctx: Pair<String, Any?>) {
        val o = JSONObject()
        ctx.forEach { (k, v) -> o.put(k, v) }
        engine.log(level, event, o.toString())
    }

    companion object {
        lateinit var instance: App
            private set
    }
}
