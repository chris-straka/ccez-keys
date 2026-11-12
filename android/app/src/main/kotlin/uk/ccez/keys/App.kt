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
    init {
        instance = this
    }

    /** Started on first use: `keys` can call in before onCreate has run. */
    val engine: Engine by lazy { Engine(filesDir.resolve("keys").path) }

    /** Text camera text asked to put in the field the keyboard comes back to. */
    @Volatile var pendingInsert: String? = null

    /** Bumped whenever a setting changes (from the panel or `keys`). */
    val settingsVersion = MutableStateFlow(0)

    /** Background work. A failure in one job is logged, never a crash of the keyboard. */
    val scope = CoroutineScope(
        SupervisorJob() + Dispatchers.IO + kotlinx.coroutines.CoroutineExceptionHandler { _, e ->
            runCatching { log("error", "job-failed", "error" to e.toString(), "at" to e.stackTrace.take(6).joinToString(" < ")) }
        },
    )

    override fun onCreate() {
        super.onCreate()
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { t, e ->
            runCatching { log("error", "crash", "thread" to t.name, "error" to e.toString(), "at" to e.stackTrace.take(8).joinToString(" < ")) }
            previous?.uncaughtException(t, e)
        }
        log("info", "start", "version" to BuildConfig.VERSION_NAME)
        keepMainThreadAlive()
        loadLanguages()
    }

    /** Loads the languages most likely typed: the default, the current one, the first learning ones. */
    fun loadLanguages() = scope.launch {
        val langs = engine.typingLanguages()
        (listOf(engine.currentLanguage()) + langs.take(4)).distinct().forEach { ensureWords(it) }
    }

    /** Languages with words in memory, least recently used first. */
    private val loaded = LinkedHashSet<String>()

    /** A language's word list, once. Safe from any thread. */
    @Synchronized
    fun ensureWords(lang: String) {
        loaded.remove(lang)
        loaded.add(lang)
        // At most six word lists (about 6 MB each): the oldest goes.
        while (loaded.size > 6) {
            val old = loaded.first { it != lang && it != engine.currentLanguage() }
            loaded.remove(old)
            engine.unload(old)
        }
        if (engine.loaded(lang)) return
        val started = System.nanoTime()
        val text = runCatching { assets.open("words/$lang.txt").bufferedReader().readText() }.getOrNull()
        if (text == null) {
            log("warn", "no-words", "lang" to lang)
            return
        }
        val extra = runCatching { assets.open("words/$lang-extra.txt").bufferedReader().readText() }.getOrDefault("")
        val n = engine.loadWords(lang, text + "\n" + extra)
        // The words likely next (from Tatoeba), where there are some.
        runCatching { assets.open("next/$lang.txt").bufferedReader().readText() }.getOrNull()?.let { engine.loadNext(lang, it) }
        log("info", "words", "lang" to lang, "count" to n.toLong(), "ms" to (System.nanoTime() - started) / 1_000_000)
    }

    private val flags = java.util.concurrent.ConcurrentHashMap<String, Boolean>()
    private val lists = java.util.concurrent.ConcurrentHashMap<String, List<String>>()
    private val texts = java.util.concurrent.ConcurrentHashMap<String, String>()
    private val numbers = java.util.concurrent.ConcurrentHashMap<String, Int>()

    /** Settings as Kotlin keeps them between changes: hot paths read these, not the core over JNI. */
    fun flag(name: String): Boolean = flags.getOrPut(name) { engine.settingBool(name) }
    fun strings(name: String): List<String> = lists.getOrPut(name) { engine.settingStrings(name) }
    fun text(name: String): String = texts.getOrPut(name) { engine.setting(name).trim('"') }
    fun number(name: String): Int = numbers.getOrPut(name) { engine.setting(name).toIntOrNull() ?: 0 }

    fun settingsChanged() {
        flags.clear(); lists.clear(); texts.clear(); numbers.clear()
        settingsVersion.value += 1
        loadLanguages()
    }

    /**
     * A keyboard that crashes leaves the phone with none: an error on the
     * main thread is logged (`keys log`, event "recovered") and the main
     * loop carries on. Three in ten seconds: let it crash, something's stuck.
     */
    private fun keepMainThreadAlive() {
        val recent = ArrayDeque<Long>()
        android.os.Handler(android.os.Looper.getMainLooper()).post {
            while (true) {
                try {
                    android.os.Looper.loop()
                } catch (e: Throwable) {
                    val now = System.currentTimeMillis()
                    recent.addLast(now)
                    while (recent.isNotEmpty() && now - recent.first() > 10_000) recent.removeFirst()
                    runCatching {
                        // The innermost cause says what went wrong (the outer one is Android's wrapper).
                        val root = generateSequence(e) { it.cause }.last()
                        log("error", "recovered", "error" to e.toString(), "cause" to root.toString(), "at" to root.stackTrace.take(10).joinToString(" < "))
                    }
                    if (recent.size >= 3) throw e
                    onRecovered?.invoke()
                }
            }
        }
    }

    /** The keys on screen now, as JSON (`keys keymap`); set by the keyboard while it's up. */
    @Volatile var keymap: (() -> String)? = null

    /** Hides the clipboard chip (after the AI key was pasted from it). */
    @Volatile var clearClipChip: (() -> Unit)? = null

    /** The keyboard resets its typing state after a recovered error. */
    @Volatile var onRecovered: (() -> Unit)? = null

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

/**
 * Counts for one field, logged when it closes as "typing-speed": how long
 * keys took to handle (main thread), translations run and AI calls made.
 * Never what was typed.
 */
object Stats {
    private val keyNs = ArrayList<Long>()
    val translations = java.util.concurrent.atomic.AtomicInteger()
    val ai = java.util.concurrent.atomic.AtomicInteger()

    @Synchronized fun key(ns: Long) { if (keyNs.size < 5000) keyNs += ns }

    @Synchronized fun flush() {
        val t = translations.getAndSet(0)
        val a = ai.getAndSet(0)
        if (keyNs.isEmpty()) return
        val ms = keyNs.sorted().map { it / 1_000_000.0 }
        keyNs.clear()
        fun pct(p: Double) = "%.1f".format(ms[((ms.size - 1) * p).toInt()])
        App.instance.log("info", "typing-speed", "keys" to ms.size, "p50_ms" to pct(0.5), "p95_ms" to pct(0.95), "max_ms" to pct(1.0), "translations" to t, "ai" to a)
    }
}
