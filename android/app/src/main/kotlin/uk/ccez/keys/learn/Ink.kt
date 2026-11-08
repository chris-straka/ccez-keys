package uk.ccez.keys.learn

import com.google.mlkit.common.model.DownloadConditions
import com.google.mlkit.common.model.RemoteModelManager
import com.google.mlkit.vision.digitalink.DigitalInkRecognition
import com.google.mlkit.vision.digitalink.DigitalInkRecognitionModel
import com.google.mlkit.vision.digitalink.DigitalInkRecognitionModelIdentifier
import com.google.mlkit.vision.digitalink.DigitalInkRecognizerOptions
import com.google.mlkit.vision.digitalink.Ink
import uk.ccez.keys.App

/** A character drawn with a finger, read by ML Kit (its model downloads on first use). */
object InkReader {
    private fun model(lang: String): DigitalInkRecognitionModel? {
        val tag = when (lang) { "ja" -> "ja"; "zh" -> "zh-Hani-CN"; else -> lang }
        val id = runCatching { DigitalInkRecognitionModelIdentifier.fromLanguageTag(tag) }.getOrNull() ?: return null
        return DigitalInkRecognitionModel.builder(id).build()
    }

    /** Strokes as lists of (x, y, time ms). Up to eight readings, likeliest first. */
    suspend fun read(lang: String, strokes: List<List<Triple<Float, Float, Long>>>): List<String> {
        val m = model(lang) ?: return emptyList()
        return runCatching {
            val manager = RemoteModelManager.getInstance()
            if (!manager.isModelDownloaded(m).await()) {
                App.instance.log("info", "ink-model", "lang" to lang)
                manager.download(m, DownloadConditions.Builder().build()).await()
            }
            val ink = Ink.builder()
            strokes.forEach { s ->
                val b = Ink.Stroke.builder()
                s.forEach { (x, y, t) -> b.addPoint(Ink.Point.create(x, y, t)) }
                ink.addStroke(b.build())
            }
            val r = DigitalInkRecognition.getClient(DigitalInkRecognizerOptions.builder(m).build())
            try {
                r.recognize(ink.build()).await().candidates.map { it.text }.take(8)
            } finally {
                r.close()
            }
        }.onFailure { App.instance.log("warn", "ink-failed", "error" to it.toString()) }.getOrDefault(emptyList())
    }
}
