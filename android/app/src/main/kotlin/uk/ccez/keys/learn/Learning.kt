package uk.ccez.keys.learn

import android.content.Context
import android.speech.tts.TextToSpeech
import android.speech.tts.Voice
import com.google.android.gms.tasks.Task
import com.google.mlkit.common.model.DownloadConditions
import com.google.mlkit.common.model.RemoteModelManager
import com.google.mlkit.nl.translate.TranslateLanguage
import com.google.mlkit.nl.translate.TranslateRemoteModel
import com.google.mlkit.nl.translate.Translation
import com.google.mlkit.nl.translate.TranslatorOptions
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.Locale
import java.util.UUID
import java.util.zip.ZipInputStream
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import uk.ccez.keys.App

suspend fun <T> Task<T>.await(): T = suspendCancellableCoroutine { cont ->
    addOnSuccessListener { cont.resume(it) }
    addOnFailureListener { cont.resumeWithException(it) }
}

/**
 * Google's on-device translator (ML Kit): offline once a language's model
 * (about 30 MB) is on the phone. One translation at a time across the app:
 * two translators starting together abort the process (found in ccez-tube).
 */
object Translator {
    private val ONE = Mutex()
    private val have = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()
    /** One download per language: a second asker waits for the first. */
    private val fetching = java.util.concurrent.ConcurrentHashMap<String, Mutex>()
    private val cache = object : LinkedHashMap<String, String>(128, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, String>?) = size > 256
    }

    fun supports(lang: String) = TranslateLanguage.fromLanguageTag(lang) != null

    private suspend fun models(vararg langs: String) {
        val manager = RemoteModelManager.getInstance()
        for (tag in langs) {
            val lang = TranslateLanguage.fromLanguageTag(tag) ?: continue
            if (lang == TranslateLanguage.ENGLISH || lang in have) continue
            fetching.getOrPut(lang) { Mutex() }.withLock {
                if (lang in have) return@withLock
                val model = TranslateRemoteModel.Builder(lang).build()
                if (!manager.isModelDownloaded(model).await()) {
                    App.instance.log("info", "model-download", "lang" to tag)
                    manager.download(model, DownloadConditions.Builder().build()).await()
                }
                have += lang
            }
        }
    }

    /** `text` from one language into another, or null (no model yet, unsupported, failed). */
    suspend fun translate(text: String, from: String, to: String): String? {
        if (from == to) return text
        val source = TranslateLanguage.fromLanguageTag(from) ?: return null
        val target = TranslateLanguage.fromLanguageTag(to) ?: return null
        val key = "$from|$to|$text"
        synchronized(cache) { cache[key] }?.let { return it }
        return runCatching {
            models(from, to)
            val client = Translation.getClient(TranslatorOptions.Builder().setSourceLanguage(source).setTargetLanguage(target).build())
            try {
                ONE.withLock { client.translate(text).await() }
            } finally {
                client.close()
            }
        }.onFailure { App.instance.log("warn", "translate-failed", "from" to from, "to" to to, "error" to it.javaClass.simpleName) }
            .getOrNull()
            ?.also { synchronized(cache) { cache[key] = it } }
    }
}

/**
 * Dictionaries, downloaded the first time a language needs one, from the
 * public release of ccez-lang-core, checked against its SHA256SUMS, then
 * packed by the core.
 */
object Dictionaries {
    private const val BASE = "https://github.com/chris-straka/ccez-lang-core/releases/download/dictionaries/"
    private val ONE = Mutex()
    private var sums: Map<String, String>? = null
    private val failed = mutableSetOf<String>()

    private fun files(lang: String): List<String> = when (lang) {
        "zh" -> listOf("cedict.txt.gz")
        "ja" -> listOf("ja.tsv.gz", "ja-pitch.tsv.gz", "ipadic.zip")
        else -> listOf("$lang.tsv.gz")
    }

    /** Japanese and Chinese also get a converter (typing them), built from the same file. */
    private fun converts(lang: String) = lang == "ja" || lang == "zh"

    private fun complete(lang: String): Boolean {
        val e = App.instance.engine
        return e.dictionaryReady(lang) && (!converts(lang) || e.hasConverter(lang))
    }

    /** Fetches and installs `lang`'s dictionary unless it's there. True when it was installed just now. */
    suspend fun ensure(lang: String): Boolean {
        val engine = App.instance.engine
        if (complete(lang) || lang in failed) return false
        ONE.withLock {
            if (complete(lang)) return false
            val started = System.nanoTime()
            try {
                val sums = sums ?: fetchSums().also { sums = it }
                // Only the main file when just the converter is missing.
                val wanted = files(lang).filter { it in sums }.let { all -> if (engine.dictionaryReady(lang)) all.take(1) else all }
                if (wanted.isEmpty()) { failed += lang; return false }
                val dir = File(App.instance.cacheDir, "dict").apply { mkdirs() }
                for (name in wanted) {
                    val file = File(dir, name)
                    download(BASE + name, file)
                    val sha = sha256(file)
                    if (sha != sums[name]) {
                        file.delete()
                        error("checksum of $name")
                    }
                    when {
                        name == "ipadic.zip" -> unzip(file, File(engine.dictionaryDir(), "ipadic"))
                        name == "ja-pitch.tsv.gz" -> engine.installPitch(file.path)
                        else -> {
                            if (!engine.dictionaryReady(lang)) engine.installDictionary(lang, file.path)
                            if (converts(lang)) {
                                val freq = App.instance.assets.open("words/$lang.txt").bufferedReader().readText()
                                engine.buildConverter(lang, file.path, freq)
                            }
                        }
                    }
                    file.delete()
                }
                App.instance.log("info", "dictionary", "lang" to lang, "ms" to (System.nanoTime() - started) / 1_000_000)
                return true
            } catch (e: Exception) {
                failed += lang
                App.instance.log("warn", "dictionary-failed", "lang" to lang, "error" to e.toString())
                return false
            }
        }
    }

    private fun fetchSums(): Map<String, String> {
        val text = (URL(BASE + "SHA256SUMS").openConnection() as HttpURLConnection).run {
            instanceFollowRedirects = true
            connectTimeout = 15_000
            readTimeout = 30_000
            inputStream.bufferedReader().readText()
        }
        return text.lines().mapNotNull { l -> l.trim().split(Regex("\\s+"), 2).takeIf { it.size == 2 }?.let { it[1].trimStart('*') to it[0] } }.toMap()
    }

    private fun download(url: String, to: File) {
        val c = URL(url).openConnection() as HttpURLConnection
        c.instanceFollowRedirects = true
        c.connectTimeout = 15_000
        c.readTimeout = 60_000
        c.inputStream.use { input -> to.outputStream().use { input.copyTo(it) } }
    }

    private fun sha256(f: File): String {
        val md = MessageDigest.getInstance("SHA-256")
        f.inputStream().use { s -> val b = ByteArray(1 shl 16); while (true) { val n = s.read(b); if (n < 0) break; md.update(b, 0, n) } }
        return md.digest().joinToString("") { "%02x".format(it) }
    }

    private fun unzip(zip: File, into: File) {
        into.mkdirs()
        ZipInputStream(zip.inputStream()).use { z ->
            while (true) {
                val e = z.nextEntry ?: break
                val out = File(into, e.name.substringAfterLast('/'))
                if (!e.isDirectory && out.canonicalPath.startsWith(into.canonicalPath)) out.outputStream().use { z.copyTo(it) }
            }
        }
    }
}

/**
 * Says text aloud with the phone's best voice for the language (highest
 * quality, offline first), slightly slow, as ccez-tube does.
 */
object Speaker {
    private var tts: TextToSpeech? = null
    private var ready = false
    private var pending: Pair<String, String>? = null

    fun say(ctx: Context, text: String, lang: String) {
        val t = tts ?: TextToSpeech(ctx.applicationContext) { status ->
            ready = status == TextToSpeech.SUCCESS
            pending?.let { (s, l) -> if (ready) speak(s, l) }
            pending = null
        }.also { tts = it }
        if (!ready) { pending = text to lang; return }
        speak(text, lang)
    }

    private fun speak(text: String, lang: String) {
        val t = tts ?: return
        val want = Locale.forLanguageTag(lang).language
        val voice = runCatching { t.voices }.getOrNull().orEmpty()
            .filter { it.locale.language == want && TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED !in it.features.orEmpty() }
            .maxWithOrNull(compareBy<Voice>({ it.quality }, { !it.isNetworkConnectionRequired }))
            ?: return
        t.voice = voice
        t.setSpeechRate(0.9f)
        t.speak(text, TextToSpeech.QUEUE_FLUSH, null, "strip")
    }
}

/** Word statuses to and from ccez-words: at start, every five minutes, and soon after a change. */
object WordSync {
    private var last = 0L
    private var job: kotlinx.coroutines.Job? = null

    private fun from(ctx: Context): String {
        val p = ctx.getSharedPreferences("sync", Context.MODE_PRIVATE)
        val id = p.getString("id", null) ?: UUID.randomUUID().toString().take(8).also { p.edit().putString("id", it).apply() }
        return "ccez-keys:$id"
    }

    fun soon(delayMs: Long = 4_000) {
        val app = App.instance
        if (app.engine.setting("words.server").trim('"').isEmpty()) return
        job?.cancel()
        job = app.scope.launch {
            delay(delayMs)
            val r = runCatching { app.engine.syncWords(from(app)) }
            last = System.currentTimeMillis()
            r.onSuccess { n -> app.log("info", "words-sync", "came" to n.toLong()) }
                .onFailure { app.log("warn", "words-sync-failed", "error" to it.message) }
        }
    }

    /** At most every five minutes. */
    fun maybe() {
        if (System.currentTimeMillis() - last > 5 * 60_000) soon(0)
    }
}
