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
        if (value.isNullOrBlank()) { prefs(ctx).edit().remove("ai").apply(); return }
        val c = Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        val data = c.iv + c.doFinal(value.trim().toByteArray())
        prefs(ctx).edit().putString("ai", Base64.encodeToString(data, Base64.NO_WRAP)).apply()
    }

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
 * or ends a sentence, Muse checks it (once per sentence) and a violet note
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
        val learning = lang != e.setting("languages.default").trim('"') && lang in e.settingStrings("languages.learning")
        if (!allowed || !learning || !e.settingBool("ai.tips") || sentence.split(' ').count { it.isNotBlank() } < 3) return
        val key = Secrets.get(app) ?: return
        synchronized(seen) { if (seen.containsKey(sentence)) { seen[sentence]?.let { ui.tip = Tip(sentence, lang, it) }; return } }
        job = app.scope.launch {
            delay(if (ended) 300 else 1500)
            val fix = runCatching { e.correct(lang, sentence, key) }
                .onFailure { app.log("warn", "correction-failed", "error" to it.message) }
                .getOrElse { return@launch }
            synchronized(seen) { seen[sentence] = fix }
            if (fix != null) withContext(Dispatchers.Main) { ui.tip = Tip(sentence, lang, fix) }
        }
    }
}

/** One line of an Ask chat. */
data class Line(val role: String, val text: String)

/** Ask: a chat with the AI about something typed, its input typed on this keyboard. */
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
        val messages = if (history.isEmpty()) e.askStart(s.about, q) else history + ChatMessage("user", q)
        ui.ask = s.copy(lines = s.lines + Line("user", q) + Line("assistant", ""), input = "", busy = true)
        val chat = ui.ask
        job = app.scope.launch {
            val answer = runCatching {
                e.ask(messages, key, object : AnswerSink {
                    override fun text(soFar: String) {
                        app.scope.launch(Dispatchers.Main) {
                            // Only into the chat it was asked in.
                            ui.ask?.takeIf { it.about == chat?.about && it.lines.size == chat.lines.size }?.let { a -> ui.ask = a.copy(lines = a.lines.dropLast(1) + Line("assistant", soFar)) }
                        }
                    }
                })
            }
            withContext(Dispatchers.Main) {
                val a = ui.ask?.takeIf { it.about == chat?.about && it.lines.size == chat.lines.size } ?: return@withContext
                answer.onSuccess { full ->
                    history = messages + ChatMessage("assistant", full)
                    ui.ask = a.copy(lines = a.lines.dropLast(1) + Line("assistant", full), busy = false)
                }.onFailure {
                    ui.ask = a.copy(lines = a.lines.dropLast(1) + Line("assistant", "Couldn't reach the AI."), busy = false)
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
        return "About: ${s.about}\n\n" + s.lines.joinToString("\n\n") { (if (it.role == "user") "Me: " else "Muse: ") + it.text }
    }
}

data class AskState(val about: String, val lines: List<Line>, val input: String, val busy: Boolean)
