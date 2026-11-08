package uk.ccez.keys

import android.Manifest
import android.content.ContentUris
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageCapture
import androidx.camera.core.ImageCaptureException
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import com.google.mlkit.nl.languageid.LanguageIdentification
import com.google.mlkit.vision.common.InputImage
import com.google.mlkit.vision.text.TextRecognition
import com.google.mlkit.vision.text.chinese.ChineseTextRecognizerOptions
import com.google.mlkit.vision.text.japanese.JapaneseTextRecognizerOptions
import com.google.mlkit.vision.text.korean.KoreanTextRecognizerOptions
import com.google.mlkit.vision.text.latin.TextRecognizerOptions
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uk.ccez.keys.core.StripWord
import uk.ccez.keys.core.WordStatus
import uk.ccez.keys.learn.Dictionaries
import uk.ccez.keys.learn.Speaker
import uk.ccez.keys.learn.Translator
import uk.ccez.keys.learn.await
import uk.ccez.keys.ui.Langs

/**
 * Camera text: point at a sign, a menu or another screen (or read the
 * latest screenshot); the text comes back with each word known, learning
 * or new, and its translation. "Into message" types it into the field the
 * keyboard was in.
 */
class CameraActivity : ComponentActivity() {
    private val scope: CoroutineScope = MainScope()
    private var capture: ImageCapture? = null

    private data class Read(val lang: String, val lines: List<List<StripWord>>, val text: String, val translation: String?)

    private var shot by mutableStateOf<Bitmap?>(null)
    private var read by mutableStateOf<Read?>(null)
    private var busy by mutableStateOf(false)
    private var meaning by mutableStateOf<Pair<String, String>?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        if (checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.CAMERA), 1)
        }
        setContent { Screen() }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == 2 && grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) latestScreenshot()
        if (requestCode == 1) setContent { Screen() }
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    @OptIn(ExperimentalFoundationApi::class)
    @Composable
    private fun Screen() {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            val b = shot
            if (b == null) {
                if (checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
                    AndroidView(factory = { ctx -> PreviewView(ctx).also { bindCamera(it) } }, modifier = Modifier.fillMaxSize())
                }
            } else {
                Image(b.asImageBitmap(), null, modifier = Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
            }
            Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().safeDrawingPadding()) {
                read?.let { Result(it) }
                Row(
                    Modifier.fillMaxWidth().padding(20.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    // The latest screenshot instead of the camera.
                    Box(
                        Modifier.size(44.dp).clip(RoundedCornerShape(8.dp)).background(Color(0xFF2B2B2E)).combinedClickable(onClick = { latestScreenshot() }),
                        contentAlignment = Alignment.Center,
                    ) { Text("▣", color = Colors.muted, fontSize = 20.sp) }
                    Box(
                        Modifier.size(70.dp).clip(RoundedCornerShape(35.dp)).background(if (busy) Colors.dim else Colors.fg)
                            .combinedClickable(onClick = { if (shot != null) { shot = null; read = null } else take() }),
                    )
                    Box(
                        Modifier.size(44.dp).clip(RoundedCornerShape(22.dp)).background(Color(0xFF2B2B2E)).combinedClickable(onClick = { finish() }),
                        contentAlignment = Alignment.Center,
                    ) { Text("✕", color = Colors.fg, fontSize = 18.sp) }
                }
            }
        }
    }

    @OptIn(ExperimentalFoundationApi::class)
    @Composable
    private fun Result(r: Read) {
        Column(
            Modifier.fillMaxWidth().padding(horizontal = 10.dp).clip(RoundedCornerShape(16.dp)).background(Colors.bg)
                .padding(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Column(Modifier.fillMaxWidth().height(170.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                r.lines.forEach { line ->
                    FlowRow {
                        line.forEach { w ->
                            if (!w.word && w.text.isBlank()) {
                                Spacer(Modifier.width(5.dp))
                            } else {
                                Text(
                                    w.text,
                                    fontSize = 17.sp,
                                    color = if (w.word && w.status == WordStatus.KNOWN) Colors.dim else Colors.fg,
                                    textDecoration = if (w.word && w.status == WordStatus.LEARNING) TextDecoration.Underline else null,
                                    modifier = if (!w.word) Modifier else Modifier.combinedClickable(
                                        onClick = { meaningOf(r.lang, w) },
                                        onLongClick = { App.instance.engine.setWordStatus(r.lang, w.lemma.ifEmpty { w.text }, WordStatus.LEARNING); reRead() },
                                    ),
                                )
                            }
                        }
                    }
                }
            }
            meaning?.let { (word, m) -> Text("$word  ·  $m", color = Colors.accent, fontSize = 13.sp) }
            r.translation?.let { Text(it, color = Colors.muted, fontSize = 13.sp, maxLines = 3) }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(Langs.of(this@CameraActivity, r.lang)?.flag ?: r.lang, fontSize = 20.sp)
                Box(
                    Modifier.size(38.dp).clip(RoundedCornerShape(19.dp)).background(Colors.key).combinedClickable(onClick = { Speaker.say(this@CameraActivity, r.text, r.lang) }),
                    contentAlignment = Alignment.Center,
                ) { Text("🔊", fontSize = 15.sp) }
                Spacer(Modifier.weight(1f))
                Text(
                    "Into message",
                    color = Color(Colors.ENTER_FG),
                    fontSize = 13.5.sp,
                    fontWeight = FontWeight.Medium,
                    modifier = Modifier.clip(RoundedCornerShape(19.dp)).background(Color(Colors.ENTER))
                        .combinedClickable(onClick = { App.instance.pendingInsert = r.text; finish() }).padding(horizontal = 16.dp, vertical = 9.dp),
                )
            }
        }
    }

    private fun bindCamera(view: PreviewView) {
        val future = ProcessCameraProvider.getInstance(this)
        future.addListener({
            runCatching {
                val provider = future.get()
                val preview = Preview.Builder().build().also { it.surfaceProvider = view.surfaceProvider }
                // About 2 MP: plenty for text, and no 50 MP bitmap in memory.
                val size = androidx.camera.core.resolutionselector.ResolutionSelector.Builder()
                    .setResolutionStrategy(
                        androidx.camera.core.resolutionselector.ResolutionStrategy(
                            android.util.Size(1920, 1080),
                            androidx.camera.core.resolutionselector.ResolutionStrategy.FALLBACK_RULE_CLOSEST_LOWER_THEN_HIGHER,
                        ),
                    ).build()
                val cap = ImageCapture.Builder().setCaptureMode(ImageCapture.CAPTURE_MODE_MINIMIZE_LATENCY).setResolutionSelector(size).build()
                capture = cap
                provider.unbindAll()
                provider.bindToLifecycle(this, CameraSelector.DEFAULT_BACK_CAMERA, preview, cap)
            }.onFailure { App.instance.log("warn", "camera", "error" to it.toString()) }
        }, ContextCompat.getMainExecutor(this))
    }

    private fun take() {
        val cap = capture ?: return
        busy = true
        cap.takePicture(ContextCompat.getMainExecutor(this), object : ImageCapture.OnImageCapturedCallback() {
            override fun onCaptureSuccess(image: ImageProxy) {
                val rotation = image.imageInfo.rotationDegrees
                val bmp = runCatching {
                    image.toBitmap().let { b ->
                        if (rotation == 0) b else Bitmap.createBitmap(b, 0, 0, b.width, b.height, android.graphics.Matrix().apply { postRotate(rotation.toFloat()) }, true)
                    }
                }.getOrNull()
                image.close()
                if (bmp == null) { busy = false; return }
                shot = bmp
                recognize(InputImage.fromBitmap(bmp, 0))
            }

            override fun onError(e: ImageCaptureException) {
                busy = false
                App.instance.log("warn", "camera-capture", "error" to e.imageCaptureError)
            }
        })
    }

    private fun latestScreenshot() {
        val perm = Manifest.permission.READ_MEDIA_IMAGES
        if (checkSelfPermission(perm) != PackageManager.PERMISSION_GRANTED) { requestPermissions(arrayOf(perm), 2); return }
        val uri: Uri = contentResolver.query(
            MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
            arrayOf(MediaStore.Images.Media._ID),
            "${MediaStore.Images.Media.RELATIVE_PATH} LIKE ?",
            arrayOf("%Screenshots%"),
            "${MediaStore.Images.Media.DATE_ADDED} DESC",
        )?.use { c -> if (c.moveToFirst()) ContentUris.withAppendedId(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, c.getLong(0)) else null } ?: return
        busy = true
        val bmp = runCatching { contentResolver.openInputStream(uri)?.use { android.graphics.BitmapFactory.decodeStream(it) } }.getOrNull() ?: run { busy = false; return }
        shot = bmp
        recognize(InputImage.fromBitmap(bmp, 0))
    }

    private var lastText = ""
    private var lastLang = ""

    /** Every script the phone reads, keeping whichever found the most. */
    private fun recognize(image: InputImage) {
        scope.launch {
            val clients = listOf(
                TextRecognition.getClient(TextRecognizerOptions.DEFAULT_OPTIONS),
                TextRecognition.getClient(JapaneseTextRecognizerOptions.Builder().build()),
                TextRecognition.getClient(ChineseTextRecognizerOptions.Builder().build()),
                TextRecognition.getClient(KoreanTextRecognizerOptions.Builder().build()),
            )
            val results = clients.map { c -> runCatching { c.process(image).await() }.getOrNull().also { c.close() } }
            // Latin unless a CJK reader found a real share of its own script
            // (they read stray "characters" into icons).
            val latin = results[0]
            val cjkBest = results.drop(1).filterNotNull().maxByOrNull { cjk(it.text) }
            val best = if (cjkBest != null && cjk(cjkBest.text) >= 6 && cjk(cjkBest.text) * 3 >= cjkBest.text.count { !it.isWhitespace() }) cjkBest else latin ?: cjkBest
            val text = best?.textBlocks?.joinToString("\n") { b -> b.lines.joinToString("\n") { it.text } }.orEmpty().trim()
            if (text.isEmpty()) { busy = false; read = null; return@launch }
            val lang = languageOf(text)
            lastText = text
            lastLang = lang
            show()
        }
    }

    private fun reRead() = scope.launch { show() }

    private suspend fun show() {
        val e = App.instance.engine
        val lang = lastLang
        val text = lastText
        withContext(Dispatchers.IO) { Dictionaries.ensure(lang) }
        val lines = text.lines().map { e.stripWords(lang, it) }
        val def = e.setting("languages.default").trim('"')
        val translation = if (lang != def) Translator.translate(text.replace('\n', ' '), lang, def) else null
        read = Read(lang, lines, text, translation)
        busy = false
    }

    private fun cjk(t: String) = t.count { it in '぀'..'ヿ' || it in '一'..'鿿' || it in '가'..'힯' }

    /** By the script most of the letters are in; Latin text by ML Kit's language ID. */
    private suspend fun languageOf(text: String): String {
        val letters = text.count { it.isLetter() }.coerceAtLeast(1)
        val kana = text.count { it in '぀'..'ヿ' }
        val hangul = text.count { it in '가'..'힯' }
        val han = text.count { it in '一'..'鿿' }
        if (kana > 0 && (kana + han) * 3 >= letters) return "ja"
        if (hangul * 3 >= letters) return "ko"
        if (han * 3 >= letters) return "zh"
        val id = runCatching { LanguageIdentification.getClient().use { it.identifyLanguage(text).await() } }.getOrNull()
        return if (id == null || id == "und") App.instance.engine.currentLanguage() else id
    }

    private fun meaningOf(lang: String, w: StripWord) {
        scope.launch {
            val e = App.instance.engine
            val card = e.wordCard(lang, w.text, w.lemma)
            val m = card?.meanings?.take(2)?.joinToString("; ")?.ifEmpty { null }
                ?: Translator.translate(w.text, lang, e.setting("languages.default").trim('"'))
            meaning = (card?.lemma?.ifEmpty { null } ?: w.text) to (m ?: "")
        }
    }
}
