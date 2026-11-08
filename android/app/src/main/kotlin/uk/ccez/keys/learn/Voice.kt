package uk.ccez.keys.learn

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Bundle
import android.os.IBinder
import android.os.ParcelFileDescriptor
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import java.io.ByteArrayOutputStream
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import uk.ccez.keys.App
import uk.ccez.whisper.IWhisper

/** Recognisers want a region ("fr" alone is refused as unsupported). */
private val REGION = mapOf(
    "en" to "en-US", "fr" to "fr-FR", "de" to "de-DE", "es" to "es-ES", "zh" to "zh-CN", "ja" to "ja-JP", "ko" to "ko-KR",
    "pt" to "pt-PT", "ar" to "ar-SA", "hi" to "hi-IN", "ru" to "ru-RU", "pl" to "pl-PL", "id" to "id-ID", "tr" to "tr-TR",
    "it" to "it-IT", "no" to "nb-NO", "cs" to "cs-CZ", "el" to "el-GR", "fa" to "fa-IR", "th" to "th-TH", "vi" to "vi-VN",
    "ro" to "ro-RO", "bg" to "bg-BG", "hu" to "hu-HU", "hy" to "hy-AM", "nl" to "nl-NL", "sv" to "sv-SE", "uk" to "uk-UA", "he" to "he-IL",
)

/** What the voice panel shows. */
data class VoiceState(val lang: String, val heard: String, val listening: Boolean, val whisper: Boolean, val problem: String? = null)

/**
 * Dictation. A tap: Android's recogniser (on the phone when it can),
 * words appearing as they're heard. Held: the sound is recorded and
 * ccez-whisper hears it when the button is let go (slower, cleaner).
 */
class Voice(private val ctx: Context, private val onState: (VoiceState?) -> Unit, private val onText: (String) -> Unit) {
    private val app = App.instance
    private var recognizer: SpeechRecognizer? = null
    private var recording: AudioRecord? = null
    private var buffer: ByteArrayOutputStream? = null
    private var state: VoiceState? = null

    fun allowed() = ctx.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

    private fun set(s: VoiceState?) {
        state = s
        onState(s)
    }

    /** `onDevice`: Android's own recogniser on the phone; without its language pack, Google's online one. */
    fun start(lang: String, onDevice: Boolean = true) {
        stop()
        if (!allowed()) { set(VoiceState(lang, "", false, false, problem = "mic")); return }
        val local = onDevice && android.os.Build.VERSION.SDK_INT >= 31 && SpeechRecognizer.isOnDeviceRecognitionAvailable(ctx)
        val r = if (local) SpeechRecognizer.createOnDeviceSpeechRecognizer(ctx) else SpeechRecognizer.createSpeechRecognizer(ctx)
        recognizer = r
        set(VoiceState(lang, "", listening = true, whisper = false))
        r.setRecognitionListener(object : RecognitionListener {
            override fun onPartialResults(b: Bundle?) {
                val t = b?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull().orEmpty()
                state?.let { set(it.copy(heard = t)) }
            }
            override fun onResults(b: Bundle?) {
                val t = b?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull().orEmpty()
                if (t.isNotBlank()) onText(t)
                finish()
            }
            override fun onError(error: Int) {
                app.log("warn", "dictation", "error" to error, "on_device" to local)
                val noPack = error == SpeechRecognizer.ERROR_LANGUAGE_NOT_SUPPORTED || error == SpeechRecognizer.ERROR_LANGUAGE_UNAVAILABLE
                if (noPack) {
                    // Ask Android for the language's offline pack; held dictation (Whisper) works meanwhile.
                    if (android.os.Build.VERSION.SDK_INT >= 33) runCatching { recognizer?.triggerModelDownload(intent(lang)) }
                    if (local) { start(lang, onDevice = false); return }
                    state?.let { set(it.copy(listening = false, problem = "download")) }
                    recognizer?.destroy()
                    recognizer = null
                    return
                }
                state?.let { set(it.copy(listening = false, problem = if (error == SpeechRecognizer.ERROR_NO_MATCH || error == SpeechRecognizer.ERROR_SPEECH_TIMEOUT) "none" else "error")) }
                recognizer?.destroy()
                recognizer = null
            }
            override fun onReadyForSpeech(p: Bundle?) {}
            override fun onBeginningOfSpeech() {}
            override fun onRmsChanged(rms: Float) {}
            override fun onBufferReceived(b: ByteArray?) {}
            override fun onEndOfSpeech() {}
            override fun onEvent(t: Int, p: Bundle?) {}
        })
        r.startListening(intent(lang))
    }

    private fun intent(lang: String) = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
        .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
        .putExtra(RecognizerIntent.EXTRA_LANGUAGE, REGION[lang] ?: Locale.forLanguageTag(lang).toLanguageTag())
        .putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
        .putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)

    /** Held mic: record until let go, then ccez-whisper. */
    fun record(lang: String) {
        stop()
        if (!allowed()) { set(VoiceState(lang, "", false, true, problem = "mic")); return }
        val rate = 16_000
        val size = AudioRecord.getMinBufferSize(rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT).coerceAtLeast(rate)
        val rec = try {
            AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, rate, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, size)
        } catch (e: SecurityException) {
            set(VoiceState(lang, "", false, true, problem = "mic")); return
        }
        if (rec.state != AudioRecord.STATE_INITIALIZED) {
            rec.release()
            set(VoiceState(lang, "", false, true, problem = "error")); return
        }
        val out = ByteArrayOutputStream()
        recording = rec
        buffer = out
        set(VoiceState(lang, "", listening = true, whisper = true))
        runCatching { rec.startRecording() }.onFailure {
            recording = null; buffer = null; rec.release()
            set(VoiceState(lang, "", false, true, problem = "error")); return
        }
        app.scope.launch(Dispatchers.IO) {
            val b = ByteArray(size)
            while (recording === rec) {
                val n = rec.read(b, 0, b.size)
                if (n < 0) break // released, or the mic was taken
                if (n > 0) synchronized(out) { out.write(b, 0, n) }
                // Ten minutes at most.
                if (out.size() > rate * 2 * 600) break
            }
        }
    }

    /** Let go of a held mic: hand the sound to ccez-whisper. */
    fun release() {
        val rec = recording ?: return
        recording = null
        runCatching { rec.stop() }
        rec.release()
        val sound = buffer?.let { synchronized(it) { it.toByteArray() } } ?: return
        buffer = null
        val lang = state?.lang ?: return
        // Whisper invents words in silence ("Thank you."): nothing loud enough, nothing sent.
        if (loudest(sound) < 600) {
            app.log("info", "whisper-silent", "ms" to sound.size / 32)
            state?.let { set(it.copy(listening = false, problem = "none")) }
            return
        }
        state?.let { set(it.copy(listening = false, heard = "…")) }
        app.scope.launch {
            val text = withContext(Dispatchers.IO) { whisper(sound, lang) }
            withContext(Dispatchers.Main) {
                if (text == null) {
                    state?.let { set(it.copy(problem = "whisper")) }
                } else {
                    if (text.isNotBlank()) onText(text)
                    finish()
                }
            }
        }
    }

    /** The loudest 16-bit sample. */
    private fun loudest(pcm: ByteArray): Int {
        var max = 0
        var i = 0
        while (i + 1 < pcm.size) {
            val v = kotlin.math.abs(((pcm[i + 1].toInt() shl 8) or (pcm[i].toInt() and 0xff)).toShort().toInt())
            if (v > max) max = v
            i += 2
        }
        return max
    }

    private suspend fun whisper(pcm: ByteArray, lang: String): String? {
        val (service, conn) = kotlinx.coroutines.withTimeoutOrNull(10_000) { bind() } ?: return null
        if (service == null) { conn?.let { runCatching { ctx.unbindService(it) } }; return null }
        val pipe = ParcelFileDescriptor.createPipe()
        return try {
            app.scope.launch(Dispatchers.IO) {
                // Whisper may close its end early (an error): never a crash here.
                runCatching { ParcelFileDescriptor.AutoCloseOutputStream(pipe[1]).use { it.write(pcm) } }
            }
            val json = service.hear(pipe[0], lang) ?: return null
            val segs = JSONObject(json).getJSONArray("segments")
            (0 until segs.length()).joinToString(" ") { segs.getJSONObject(it).getString("text").trim() }.trim()
        } catch (e: Exception) {
            app.log("warn", "whisper", "error" to e.toString())
            null
        } finally {
            runCatching { pipe[0].close() }
            conn?.let { runCatching { ctx.unbindService(it) } }
        }
    }

    /** The service and its connection (to unbind); a null service when it isn't installed. */
    private suspend fun bind(): Pair<IWhisper?, ServiceConnection?> = kotlinx.coroutines.suspendCancellableCoroutine { cont ->
        val conn = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                if (cont.isActive) cont.resumeWith(Result.success(IWhisper.Stub.asInterface(binder) to this))
            }
            override fun onServiceDisconnected(name: ComponentName?) {}
        }
        val ok = runCatching {
            ctx.bindService(Intent("uk.ccez.whisper.HEAR").setPackage("uk.ccez.whisper"), conn, Context.BIND_AUTO_CREATE)
        }.getOrDefault(false)
        if (!ok) {
            runCatching { ctx.unbindService(conn) }
            if (cont.isActive) cont.resumeWith(Result.success(null to null))
        }
        cont.invokeOnCancellation { runCatching { ctx.unbindService(conn) } }
    }

    /** Live dictation: stop listening and use what was heard. */
    fun stopListening() {
        recognizer?.stopListening() ?: finish()
    }

    fun stop() {
        recognizer?.let { runCatching { it.cancel() }; it.destroy() }
        recognizer = null
        recording?.let { runCatching { it.stop() }; it.release() }
        recording = null
        buffer = null
    }

    private fun finish() {
        stop()
        set(null)
    }

    fun close() = finish()
}
