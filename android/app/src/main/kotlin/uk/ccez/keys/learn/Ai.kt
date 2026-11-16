package uk.ccez.keys.learn

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uk.ccez.keys.App
import uk.ccez.keys.UiState
import uk.ccez.keys.core.AnswerSink
import uk.ccez.keys.core.AskSource
import uk.ccez.keys.core.ChatMessage
import uk.ccez.keys.core.Fix

/** The AI key, encrypted with a key that never leaves the Android Keystore. */
object Secrets {
    private const val ALIAS = "ccez-keys-ai"

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return gen.generateKey()
    }

    private fun prefs(ctx: Context) = ctx.getSharedPreferences("secrets", Context.MODE_PRIVATE)

    /** Decrypted once (each Keystore decrypt is a call to the system, too slow per keystroke). */
    @Volatile private var cached: String? = null
    @Volatile private var loaded = false

    fun set(ctx: Context, value: String?) {
        cached = value?.trim()?.ifEmpty { null }
        loaded = true
        // Bumped on every change: apps given the key (keyring/) see it's new.
        val next = version(ctx) + 1
        if (value.isNullOrBlank()) { prefs(ctx).edit().remove("ai").putInt("version", next).apply(); return }
        val c = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val data = c.iv + c.doFinal(value.trim().toByteArray())
        prefs(ctx).edit().putString("ai", Base64.encodeToString(data, Base64.NO_WRAP)).putInt("version", next).apply()
    }

    fun version(ctx: Context): Int = prefs(ctx).getInt("version", 1)

    fun get(ctx: Context): String? {
        if (loaded) return cached
        val v = runCatching {
            val data = Base64.decode(prefs(ctx).getString("ai", null) ?: return@runCatching null, Base64.NO_WRAP)
            val c = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, data, 0, 12)) }
            String(c.doFinal(data, 12, data.size - 12))
        }.getOrNull()
        cached = v
        loaded = true
        return v
    }

    /** "••••abcd", for Settings. */
    fun hint(ctx: Context): String? = get(ctx)?.takeLast(4)?.let { "••••$it" }
}

/** A correction shown over the strip. */
data class Tip(val sentence: String, val lang: String, val fix: Fix)

/**
 * Corrections while a learning language is typed: when the typist pauses
 * or ends a sentence, the AI checks it (once per sentence) and a violet note
 * shows the fix the way ccez-llm does. Typing on folds it away.
 */
class Tips(private val ui: UiState) {
    private val app = App.instance
    private var job: Job? = null
    private val seen = object : LinkedHashMap<String, Fix?>(32, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Fix?>?) = size > 64
    }

    fun update(sentence: String, lang: String, allowed: Boolean, ended: Boolean) {
        job?.cancel()
        val tip = ui.tip
        if (tip != null && tip.sentence != sentence) ui.tip = null
        val e = app.engine
        val learning = lang != app.text("languages.default") && lang in app.strings("languages.learning")
        if (!allowed || !learning || !app.flag("ai.tips") || sentence.split(' ').count { it.isNotBlank() } < 3) return
        val key = Secrets.get(app) ?: return
        synchronized(seen) { if (seen.containsKey(sentence)) { seen[sentence]?.let { ui.tip = Tip(sentence, lang, it) }; return } }
        job = app.scope.launch {
            delay(if (ended) 300 else 1500)
            val fix = runCatching { uk.ccez.keys.Stats.ai.incrementAndGet(); e.correct(lang, sentence, key) }
                .onFailure { app.log("warn", "correction-failed", "error" to it.message) }
                .getOrElse { return@launch }
            synchronized(seen) { seen[sentence] = fix }
            if (fix != null) withContext(Dispatchers.Main) { ui.tip = Tip(sentence, lang, fix) }
        }
    }
}

/**
 * One line of an Ask chat; an answer brings what it looked up on the web
 * ([searches], [sources]) and, while it comes, what it's looking up now.
 */
data class Line(
    val role: String,
    val text: String,
    val searches: List<String> = emptyList(),
    val sources: List<AskSource> = emptyList(),
    val looking: String? = null,
)

/**
 * Ask: a chat with the AI about something typed, its input typed on this
 * keyboard. DeepSeek thinks lightly and may search the web or read a link
 * pasted in; what it looked up shows under the answer.
 */
class Ask(private val ui: UiState) {
    private val app = App.instance
    private var history: List<ChatMessage> = emptyList()
    private var job: kotlinx.coroutines.Job? = null

    fun open(about: String) {
        ui.ask = AskState(about = about, lines = emptyList(), input = "", busy = false)
    }

    fun send() {
        val s = ui.ask ?: return
        val q = s.input.trim()
        if (q.isEmpty() || s.busy) return
        val key = Secrets.get(app)
        if (key == null) {
            ui.ask = s.copy(lines = s.lines + Line("user", q) + Line("assistant", "No AI key yet: Settings → AI."), input = "")
            return
        }
        val e = app.engine
        val messages = if (history.isEmpty()) e.askStart(s.about, q) else history + ChatMessage("user", q, null)
        ui.ask = s.copy(lines = s.lines + Line("user", q) + Line("assistant", ""), input = "", busy = true)
        val chat = ui.ask
        // Only into the chat it was asked in.
        fun mine() = ui.ask?.takeIf { it.about == chat?.about && it.lines.size == chat.lines.size }
        fun last(change: (Line) -> Line) = app.scope.launch(Dispatchers.Main) { mine()?.let { a -> ui.ask = a.copy(lines = a.lines.dropLast(1) + change(a.lines.last())) } }
        job = app.scope.launch {
            val answer = runCatching {
                e.ask(messages, key, object : AnswerSink {
                    override fun text(soFar: String) { last { it.copy(text = soFar, looking = null) } }
                    override fun looking(what: String) { last { it.copy(looking = what) } }
                })
            }
            withContext(Dispatchers.Main) {
                val a = mine() ?: return@withContext
                answer.onSuccess { r ->
                    history = messages + ChatMessage("assistant", r.text, r.carry)
                    ui.ask = a.copy(lines = a.lines.dropLast(1) + Line("assistant", r.text, r.searches, r.sources), busy = false)
                }.onFailure {
                    // The core's words: the key refused, the balance used up, busy.
                    val why = (it as? uk.ccez.keys.core.KeysException.Bad)?.v1?.takeIf { m -> m.isNotBlank() && m.length < 200 } ?: "Couldn't reach the AI."
                    ui.ask = a.copy(lines = a.lines.dropLast(1) + Line("assistant", why), busy = false)
                }
            }
        }
    }

    fun close() {
        job?.cancel()
        job = null
        ui.ask = null
        history = emptyList()
    }

    /** The chat as text, for carrying on in ccez-llm. */
    fun transcript(): String {
        val s = ui.ask ?: return ""
        return "About: ${s.about}\n\n" + s.lines.joinToString("\n\n") { (if (it.role == "user") "Me: " else "AI: ") + it.text }
    }
}

data class AskState(val about: String, val lines: List<Line>, val input: String, val busy: Boolean)
