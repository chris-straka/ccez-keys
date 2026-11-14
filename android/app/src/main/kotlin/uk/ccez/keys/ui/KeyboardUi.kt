package uk.ccez.keys.ui

import android.content.Context
import android.view.ViewGroup
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.Crossfade
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.mandatorySystemGestures
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.systemGestures
import androidx.compose.foundation.layout.tappableElement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.foundation.layout.heightIn
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.platform.LocalContext
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.animation.core.animateFloat
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import android.speech.tts.TextToSpeech
import org.json.JSONArray
import org.json.JSONObject
import uk.ccez.keys.App
import uk.ccez.keys.Bar
import uk.ccez.keys.Colors
import uk.ccez.keys.Panel
import uk.ccez.keys.Popup
import uk.ccez.keys.Typist
import uk.ccez.keys.UiState
import uk.ccez.keys.core.KeyKind
import uk.ccez.keys.core.StripWord
import uk.ccez.keys.core.WordStatus
import uk.ccez.keys.learn.CardView
import uk.ccez.keys.learn.Speaker
import uk.ccez.keys.learn.StripView

private val BAR_H = 46.dp
/** The learning strip's band, always this tall. */
val STRIP_H = 44.dp

@Composable
fun KeyboardUi(ui: UiState, typist: Typist) {
    val version by App.instance.settingsVersion.collectAsState()
    LaunchedEffect(version) { if (version > 0) typist.refresh() }
    val d = LocalDensity.current
    val nav = WindowInsets.navigationBars.getBottom(d)
    val tap = WindowInsets.tappableElement.getBottom(d)
    val gest = WindowInsets.systemGestures.getBottom(d)
    val mand = WindowInsets.mandatorySystemGestures.getBottom(d)
    LaunchedEffect(nav, tap) { App.instance.log("info", "insets", "nav" to nav, "tappable" to tap, "gestures" to gest, "mandatory" to mand) }
    // The system draws its buttons (close keyboard) in the tappable strip,
    // which on Samsung is taller than the navigation bar it reports.
    val bottom = with(d) { maxOf(nav, tap).toDp() }
    Column(Modifier.fillMaxWidth().background(Colors.bg).padding(bottom = bottom)) {
        val ask = ui.ask
        if (ask != null) {
            AskPanel(ask, typist)
        }
        // The strip's room is always there (while the strip is on): a band that
        // came and went resized the keyboard window, which Android redraws a
        // beat late, so the keys jumped (owner, 2026-11-08). Only what's in it changes.
        val stripOn = remember(version) { App.instance.flag("learning.strip") }
        if (ask == null && ui.panel == Panel.KEYS && !ui.password && stripOn) StripBand(ui, typist)
    Box(Modifier.fillMaxWidth()) {
        Column(Modifier.fillMaxWidth()) {
            if (ui.ask == null) BarArea(ui, typist)
            AnimatedContent(ui.panel, transitionSpec = { fadeIn(tween(120)) togetherWith fadeOut(tween(90)) }, label = "panel") { panel ->
                when (panel) {
                    Panel.KEYS -> AndroidView(
                        factory = { typist.view.also { (it.parent as? ViewGroup)?.removeView(it) } },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Panel.FACE -> FacePanel(ui, typist)
                    Panel.SETTINGS -> SettingsPanel(ui, typist, version)
                    Panel.CLIPS -> ClipsPanel(ui, typist)
                    Panel.VOICE -> VoicePanel(ui, typist)
                    Panel.DRAW -> DrawPanel(ui, typist)
                }
            }
        }
        PopupLayer(ui)
        ui.card?.let { CardOverlay(it, ui, typist) }
        ui.bigPicture?.let { BigPicture(it, ui) }
    }
    }
}

/**
 * The strip's band. Its own composable (as is the bar's), so what changes
 * with every key redraws only itself, not the whole keyboard.
 */
@Composable
private fun StripBand(ui: UiState, typist: Typist) {
    Box(Modifier.fillMaxWidth().height(STRIP_H).background(Color(0xFF181818))) {
        val tip = ui.tip
        val strip = ui.strip
        val review = ui.review
        Crossfade(if (tip != null) 2 else if (strip != null) 1 else if (review != null) 3 else 0, animationSpec = tween(120), label = "strip") { which ->
            when (which) {
                3 -> review?.let { ReviewRow(it, typist) }
                2 -> tip?.let { TipNote(it, typist) }
                1 -> strip?.let { StripRow(it, typist) }
                else -> Box(Modifier.fillMaxSize())
            }
        }
    }
}

@Composable
private fun BarArea(ui: UiState, typist: Typist) {
    Crossfade(ui.bar is Bar.Words && ui.panel == Panel.KEYS, animationSpec = tween(90), label = "bar") { _ -> BarRow(ui, typist) }
}

@Composable
private fun PopupLayer(ui: UiState) {
    AnimatedVisibility(ui.popup != null, enter = fadeIn(tween(70)) + scaleIn(tween(90), initialScale = 0.85f), exit = fadeOut(tween(60))) {
        var last by remember { mutableStateOf<Popup?>(null) }
        ui.popup?.let { last = it }
        last?.let { PopupOverlay(it) }
    }
}

// --- the bar ---

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun BarRow(ui: UiState, typist: Typist) {
    val bar = ui.bar
    var drag by remember { mutableFloatStateOf(0f) }
    Row(
        Modifier.fillMaxWidth().height(BAR_H).pointerInput(Unit) {
            // A swipe either way: the buttons in place of the words, or back.
            detectHorizontalDragGestures(
                onDragEnd = { if (kotlin.math.abs(drag) > 60f && (ui.bar is Bar.Words || ui.tools)) ui.tools = !ui.tools; drag = 0f },
                onDragCancel = { drag = 0f },
            ) { _, d -> drag += d }
        },
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val removing = ui.remove
        if (removing != null && ui.panel == Panel.KEYS) {
            // A held suggestion: Remove takes it out of the suggestions; a tap elsewhere keeps it.
            Row(
                Modifier.fillMaxSize().combinedClickable(onClick = { ui.remove = null }).padding(horizontal = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(removing, color = Colors.fg, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                Row(
                    Modifier.clip(RoundedCornerShape(14.dp)).background(Colors.chip).combinedClickable(onClick = { typist.removeWord(removing) })
                        .padding(horizontal = 12.dp, vertical = 7.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Icons.trash, null, tint = Colors.muted, modifier = Modifier.size(15.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Remove", color = Colors.fg, fontSize = 13.sp)
                }
            }
            return@Row
        }
        if (bar is Bar.Convert && ui.panel == Panel.KEYS) {
            Row(Modifier.fillMaxSize().horizontalScroll(rememberScrollState()), verticalAlignment = Alignment.CenterVertically) {
                bar.words.forEachIndexed { i, c ->
                    if (i > 0) Divider()
                    Column(
                        Modifier.fillMaxHeight().widthIn(min = 64.dp).combinedClickable(onClick = { typist.pickConversion(c) }).padding(horizontal = 12.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center,
                    ) {
                        Text(c.reading, color = Colors.muted, fontSize = 9.5.sp, lineHeight = 11.sp, maxLines = 1)
                        Text(c.text, color = Colors.fg, fontSize = 17.sp, fontWeight = if (i == 0) FontWeight.SemiBold else FontWeight.Normal, maxLines = 1)
                    }
                }
            }
            return@Row
        }
        if (bar is Bar.Words && ui.panel == Panel.KEYS && !ui.tools) {
            // Swipe the bar for the buttons (owner, 2026-11-08: a button for them was
            // jarring). Something just copied: its icon, lit, opens them too.
            if (ui.clip != null && !ui.password) Box(
                Modifier.width(40.dp).fillMaxHeight().combinedClickable(onClick = { ui.tools = true }),
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.clip, "Clipboard", tint = Colors.accent, modifier = Modifier.size(18.dp)) }
            Cell(bar.left, false, bar.typed, typist, 1f)
            Divider()
            Cell(bar.mid, true, bar.typed, typist, 1.25f)
            Divider()
            val face = ui.face
            if (face != null) FaceCell(face, typist) else Cell(bar.right, false, bar.typed, typist, 1f)
            return@Row
        }
        if (!ui.password && App.instance.flag("buttons.clipboard")) {
            val on = ui.panel == Panel.CLIPS
            Box(
                Modifier.padding(start = 8.dp).size(36.dp).clip(RoundedCornerShape(8.dp)).background(if (on) Colors.line else Color.Transparent)
                    .combinedClickable(onClick = { if (on) typist.showKeys() else ui.panel = Panel.CLIPS }),
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.clip, "Clipboard", tint = if (on) Colors.fg else Colors.muted, modifier = Modifier.size(20.dp)) }
            // Right of it: something just copied, until pasted; whenever the bar is idle (an
            // empty field, or after a full stop: owner, 2026-11-12), the latest copies, newest
            // on the left (a word, a phrase, a picture or screenshot; owner, 2026-11-12: "multiple
            // clipboard items so I can tap them all"). Tap: in it goes. Hold: the clipboard.
            val clip = ui.clip
            val recent = if (ui.panel == Panel.KEYS) ui.recent else emptyList()
            val texts = recent.filterIsInstance<uk.ccez.keys.learn.Latest.Text>().map { it.text }
            Row(
                Modifier.weight(1f).horizontalScroll(rememberScrollState()),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (clip != null && clip !in texts) TextChip(clip, onTap = { typist.paste(clip); ui.clip = null }, onHold = { ui.panel = Panel.CLIPS })
                recent.forEach { item ->
                    when (item) {
                        is uk.ccez.keys.learn.Latest.Text ->
                            TextChip(item.text, onTap = { typist.paste(item.text); if (item.text == clip) ui.clip = null }, onHold = { ui.panel = Panel.CLIPS })
                        is uk.ccez.keys.learn.Latest.Picture -> {
                            val bmp = remember(item.uri) { item.thumb.asImageBitmap() }
                            androidx.compose.foundation.Image(
                                bmp,
                                "Picture",
                                contentScale = androidx.compose.ui.layout.ContentScale.Crop,
                                modifier = Modifier.padding(start = 6.dp).height(38.dp).width(28.dp).clip(RoundedCornerShape(6.dp)).background(Colors.chip)
                                    .border(1.dp, Colors.line, RoundedCornerShape(6.dp))
                                    .combinedClickable(onClick = { typist.sendShot(item.uri) }, onLongClick = { typist.holdBuzz(); ui.bigPicture = item.uri }),
                            )
                        }
                    }
                }
            }
        } else {
            Spacer(Modifier.weight(1f))
        }
        val face = ui.face
        if (face != null && ui.panel == Panel.KEYS) {
            Text(
                face,
                color = Colors.fg,
                fontSize = 13.sp,
                maxLines = 1,
                modifier = Modifier.padding(start = 8.dp).clip(RoundedCornerShape(14.dp)).background(Colors.chip)
                    .combinedClickable(onClick = { typist.insertFace(face) }).padding(horizontal = 12.dp, vertical = 8.dp),
            )
        }
        if (!ui.password && App.instance.flag("buttons.camera")) {
            Box(
                Modifier.padding(end = 4.dp).size(36.dp).clip(RoundedCornerShape(8.dp)).combinedClickable(onClick = { typist.openCamera() }),
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.cam, "Camera text", tint = Colors.muted, modifier = Modifier.size(20.dp)) }
        }
        if (!ui.password && App.instance.flag("buttons.mic")) MicButton(typist)
        val on = ui.panel == Panel.SETTINGS
        Box(
            Modifier.padding(end = 8.dp).size(36.dp).clip(RoundedCornerShape(8.dp)).background(if (on) Colors.line else Color.Transparent)
                .combinedClickable(onClick = { if (on) typist.showKeys() else ui.panel = Panel.SETTINGS }),
            contentAlignment = Alignment.Center,
        ) { Icon(Icons.gear, "Settings", tint = if (on) Colors.fg else Colors.muted, modifier = Modifier.size(20.dp)) }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun TextChip(text: String, onTap: () -> Unit, onHold: () -> Unit) {
    Text(
        text.replace('\n', ' ').trim(),
        color = Colors.fg,
        fontSize = 13.sp,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        // Several to a row: each no wider than a word or two.
        modifier = Modifier.widthIn(max = 120.dp).padding(start = 6.dp).clip(RoundedCornerShape(14.dp)).background(Colors.chip)
            .combinedClickable(onClick = onTap, onLongClick = onHold).padding(horizontal = 12.dp, vertical = 8.dp),
    )
}

/** A picture held: large over the keyboard; a tap closes it. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun androidx.compose.foundation.layout.BoxScope.BigPicture(uri: android.net.Uri, ui: UiState) {
    val ctx = LocalContext.current
    var bmp by remember(uri) { mutableStateOf<androidx.compose.ui.graphics.ImageBitmap?>(null) }
    LaunchedEffect(uri) {
        bmp = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) { uk.ccez.keys.learn.Shots.large(ctx, uri)?.asImageBitmap() }
        if (bmp == null) ui.bigPicture = null
    }
    Box(Modifier.matchParentSize().background(Color(0xF00B0B0C)).combinedClickable(onClick = { ui.bigPicture = null }), contentAlignment = Alignment.Center) {
        bmp?.let { androidx.compose.foundation.Image(it, "Picture", contentScale = androidx.compose.ui.layout.ContentScale.Fit, modifier = Modifier.fillMaxSize().padding(6.dp)) }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun RowScope.Cell(text: String?, mid: Boolean, typed: String?, typist: Typist, weight: Float) {
    Column(
        Modifier.weight(weight).fillMaxHeight().let { m ->
            // Hold: the word as typed saved to the suggestions, or another taken out.
            if (text == null) m else m.combinedClickable(onClick = { typist.pick(text, text == typed) }, onLongClick = { typist.holdWord(text, text == typed) })
        },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        if (text != null) {
            Text(
                text,
                color = Colors.fg,
                fontSize = 15.sp,
                fontWeight = if (mid) FontWeight.SemiBold else FontWeight.Normal,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun RowScope.FaceCell(face: String, typist: Typist) {
    Box(Modifier.weight(1f).fillMaxHeight().combinedClickable(onClick = { typist.insertFace(face) }), contentAlignment = Alignment.Center) {
        Text(face, color = Colors.fg, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(horizontal = 4.dp))
    }
}

@Composable
private fun Divider() = Box(Modifier.width(1.dp).height(22.dp).background(Colors.line))

// --- a held key's choices ---

@Composable
private fun PopupOverlay(p: Popup) {
    val d = LocalDensity.current
    val rows = (p.choices.size + p.cols - 1) / p.cols
    with(d) {
        Column(
            Modifier.offset(x = p.box.left.toDp(), y = BAR_H + p.box.top.toDp())
                .width(p.box.width().toDp()).height(p.box.height().toDp())
                .clip(RoundedCornerShape(8.dp)).background(Colors.popup),
        ) {
            // The first row sits nearest the key, at the bottom.
            for (r in rows - 1 downTo 0) {
                Row(Modifier.weight(1f).fillMaxWidth()) {
                    for (c in 0 until p.cols) {
                        val i = r * p.cols + c
                        Box(
                            Modifier.weight(1f).fillMaxHeight().background(if (i == p.selected) Colors.popupSel else Color.Transparent),
                            contentAlignment = Alignment.Center,
                        ) { if (i < p.choices.size) Text(p.choices[i], color = Colors.fg, fontSize = 20.sp) }
                    }
                }
            }
        }
    }
}

// --- emoji and kaomoji ---

internal object Faces {
    var kaomoji: List<List<String>> = emptyList()
    var emoji: List<List<String>> = emptyList()

    fun load(ctx: Context) {
        if (kaomoji.isNotEmpty()) return
        val o = JSONObject(ctx.assets.open("faces.json").bufferedReader().readText())
        fun lists(name: String) = o.getJSONArray(name).let { a -> (0 until a.length()).map { i -> a.getJSONArray(i).let { l -> (0 until l.length()).map(l::getString) } } }
        kaomoji = lists("kaomoji")
        emoji = lists("emoji")
    }

    fun recent(ctx: Context): List<String> =
        ctx.getSharedPreferences("faces", Context.MODE_PRIVATE).getString("recent", "").orEmpty().split('\n').filter { it.isNotEmpty() }

    fun used(ctx: Context, face: String) {
        val list = (listOf(face) + recent(ctx).filter { it != face }).take(24)
        ctx.getSharedPreferences("faces", Context.MODE_PRIVATE).edit().putString("recent", list.joinToString("\n")).apply()
    }
}

@Composable
private fun FacePanel(ui: UiState, typist: Typist) {
    val ctx = LocalContext.current
    Faces.load(ctx)
    val groups = if (ui.chatApp) Faces.kaomoji + Faces.emoji else Faces.emoji + Faces.kaomoji
    var recent by remember { mutableStateOf(Faces.recent(ctx)) }
    var tab by remember { mutableStateOf(if (recent.isEmpty()) 1 else 0) }
    val tabs = listOf("recent") + groups.map { it.first() }
    val items = if (tab == 0) recent else groups[tab - 1].drop(1)
    Column(Modifier.fillMaxWidth().height(ui.keysHeightDp.dp).padding(start = 5.dp, end = 5.dp, top = 6.dp)) {
        Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            tabs.forEachIndexed { i, t ->
                Text(
                    t,
                    color = if (i == tab) Colors.fg else Colors.muted,
                    fontSize = 12.sp,
                    maxLines = 1,
                    modifier = Modifier.clip(RoundedCornerShape(12.dp)).background(if (i == tab) Colors.key else Color.Transparent)
                        .combinedClickableCompat { tab = i }.padding(horizontal = 8.dp, vertical = 5.dp),
                )
            }
        }
        FlowRow(
            Modifier.weight(1f).fillMaxWidth().padding(vertical = 8.dp).verticalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            items.forEach { f ->
                val emoji = f.codePointCount(0, f.length) <= 3 && f.any { Character.isSurrogate(it) || it.code > 0x2000 }
                Text(
                    f,
                    color = Colors.fg,
                    fontSize = if (emoji) 24.sp else 14.sp,
                    textAlign = TextAlign.Center,
                    maxLines = 1,
                    modifier = Modifier.clip(RoundedCornerShape(8.dp)).background(if (emoji) Color.Transparent else Colors.key)
                        .combinedClickableCompat {
                            typist.insertFace(f)
                            recent = Faces.recent(ctx)
                        }
                        .padding(horizontal = if (emoji) 4.dp else 9.dp, vertical = if (emoji) 2.dp else 8.dp),
                )
            }
        }
        Row(Modifier.fillMaxWidth().height(44.dp).padding(bottom = 6.dp), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
            PanelKey("ABC", 1.3f, Colors.faceOn, Colors.accent) { typist.showKeys() }
            PanelKey(",", 0.8f) { typist.type(",") }
            PanelKey("", 3.8f, Colors.key) { typist.press(KeyKind.SPACE) }
            PanelKey(".", 0.8f) { typist.type(".") }
            PanelKey("⌫", 1.3f) { typist.press(KeyKind.BACKSPACE) }
            PanelKey("↵", 1.3f, Color(Colors.ENTER), Color(Colors.ENTER_FG)) { typist.press(KeyKind.ENTER) }
        }
    }
}

@Composable
private fun RowScope.PanelKey(label: String, weight: Float, bg: Color = Colors.mod, fg: Color = Colors.muted, onTap: () -> Unit) {
    Box(
        Modifier.weight(weight).fillMaxHeight().clip(RoundedCornerShape(6.dp)).background(bg).combinedClickableCompat(onTap),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = fg, fontSize = if (label == "↵" || label == "⌫") 22.sp else 15.sp) }
}

@OptIn(ExperimentalFoundationApi::class)
private fun Modifier.combinedClickableCompat(onClick: () -> Unit) = this.combinedClickable(onClick = onClick)

// --- dictation ---

/** Tap: Android's recogniser, live. Hold: recorded, ccez-whisper hears it when let go. */
@Composable
private fun MicButton(typist: Typist) {
    Box(
        Modifier.padding(end = 4.dp).size(36.dp).clip(RoundedCornerShape(8.dp))
            .pointerInput(Unit) {
                detectTapGestures(
                    onTap = { typist.voice.start(typist.currentLanguage()) },
                    onLongPress = { typist.voice.record(typist.currentLanguage()) },
                    onPress = {
                        tryAwaitRelease()
                        typist.voice.release()
                    },
                )
            },
        contentAlignment = Alignment.Center,
    ) { Icon(Icons.mic, "Dictation", tint = Colors.muted, modifier = Modifier.size(20.dp)) }
}

@Composable
private fun VoicePanel(ui: UiState, typist: Typist) {
    val v = ui.voice ?: return
    val ctx = LocalContext.current
    val pulse by androidx.compose.animation.core.rememberInfiniteTransition(label = "mic").animateFloat(
        initialValue = 1f,
        targetValue = 1.12f,
        animationSpec = androidx.compose.animation.core.infiniteRepeatable(tween(700), androidx.compose.animation.core.RepeatMode.Reverse),
        label = "pulse",
    )
    Column(
        Modifier.fillMaxWidth().height(ui.keysHeightDp.dp).padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(14.dp, Alignment.CenterVertically),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(Langs.of(ctx, v.lang)?.flag ?: v.lang, fontSize = 20.sp)
            if (v.whisper) Text("  Whisper", color = Colors.muted, fontSize = 12.sp)
        }
        Text(
            when (v.problem) {
                "mic" -> "Allow the microphone"
                "none" -> "Didn't catch that"
                "whisper" -> "ccez-whisper isn't ready"
                "download" -> "Downloading"
                "error" -> "Couldn't listen"
                else -> v.heard
            },
            color = if (v.problem != null) Colors.muted else Colors.fg,
            fontSize = 17.sp,
            maxLines = 3,
            textAlign = TextAlign.Center,
            modifier = Modifier.combinedClickableCompat {
                if (v.problem == "mic") ctx.startActivity(android.content.Intent(ctx, uk.ccez.keys.MainActivity::class.java).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
            },
        )
        Box(
            Modifier.size(64.dp).let { if (v.listening) it.graphicsLayer { scaleX = pulse; scaleY = pulse } else it }
                .clip(RoundedCornerShape(32.dp)).background(if (v.listening) Colors.accent else Colors.key)
                .combinedClickableCompat {
                    when {
                        v.listening && !v.whisper -> typist.voice.stopListening()
                        v.problem != null && v.problem != "mic" -> typist.voice.start(v.lang)
                        else -> typist.voice.close()
                    }
                },
            contentAlignment = Alignment.Center,
        ) { Icon(Icons.mic, "Stop", tint = if (v.listening) Color(0xFF1A1A1A) else Colors.fg, modifier = Modifier.size(28.dp)) }
    }
}

// --- drawing a character ---

@Composable
private fun DrawPanel(ui: UiState, typist: Typist) {
    val strokes = remember { androidx.compose.runtime.mutableStateListOf<List<Triple<Float, Float, Long>>>() }
    var current by remember { mutableStateOf<List<Triple<Float, Float, Long>>>(emptyList()) }
    var read by remember { mutableStateOf<List<String>>(emptyList()) }
    val lang = typist.currentLanguage()
    LaunchedEffect(strokes.size) {
        if (strokes.isEmpty()) { read = emptyList(); return@LaunchedEffect }
        kotlinx.coroutines.delay(250)
        read = uk.ccez.keys.learn.InkReader.read(lang, strokes.toList())
    }
    Column(Modifier.fillMaxWidth().height(ui.keysHeightDp.dp).padding(horizontal = 6.dp, vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(Modifier.fillMaxWidth().height(40.dp).horizontalScroll(rememberScrollState()), verticalAlignment = Alignment.CenterVertically) {
            read.forEach { c ->
                Text(
                    c,
                    color = Colors.fg,
                    fontSize = 22.sp,
                    modifier = Modifier.clip(RoundedCornerShape(8.dp)).combinedClickableCompat { typist.putDrawn(c); strokes.clear() }.padding(horizontal = 12.dp, vertical = 2.dp),
                )
            }
        }
        androidx.compose.foundation.Canvas(
            Modifier.fillMaxWidth().weight(1f).clip(RoundedCornerShape(10.dp)).background(Colors.key)
                .pointerInput(Unit) {
                    detectDragGestures(
                        onDragStart = { o -> current = listOf(Triple(o.x, o.y, System.currentTimeMillis())) },
                        onDragEnd = { if (current.size > 1) strokes.add(current); current = emptyList() },
                    ) { change, _ -> current = current + Triple(change.position.x, change.position.y, System.currentTimeMillis()) }
                },
        ) {
            val ink = Color(Colors.FG)
            (strokes + listOf(current)).forEach { s ->
                s.zipWithNext().forEach { (a, b) ->
                    drawLine(ink, androidx.compose.ui.geometry.Offset(a.first, a.second), androidx.compose.ui.geometry.Offset(b.first, b.second), strokeWidth = 6.dp.toPx(), cap = StrokeCap.Round)
                }
            }
        }
        Row(Modifier.fillMaxWidth().height(40.dp), horizontalArrangement = Arrangement.spacedBy(5.dp)) {
            PanelKey("ABC", 1.3f, Colors.faceOn, Colors.accent) { typist.showKeys() }
            PanelKey("✕", 1f) { strokes.clear() }
            PanelKey("⌫", 1.3f) { typist.press(KeyKind.BACKSPACE) }
            PanelKey("↵", 1.3f, Color(Colors.ENTER), Color(Colors.ENTER_FG)) { typist.press(KeyKind.ENTER) }
        }
    }
}

// --- clipboard history ---

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ClipsPanel(ui: UiState, typist: Typist) {
    val ctx = LocalContext.current
    var items by remember { mutableStateOf(uk.ccez.keys.learn.Clips.all(ctx)) }
    // The day's screenshots first (Android leaves them off the clipboard), then copies, two to a row.
    var shots by remember { mutableStateOf<List<Pair<android.net.Uri, androidx.compose.ui.graphics.ImageBitmap>>>(emptyList()) }
    androidx.compose.runtime.LaunchedEffect(Unit) {
        shots = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            uk.ccez.keys.learn.Shots.recent(ctx).mapNotNull { u -> uk.ccez.keys.learn.Shots.thumb(ctx, u)?.let { u to it.asImageBitmap() } }
        }
    }
    var sent by remember { mutableStateOf<android.net.Uri?>(null) }
    Column(
        Modifier.fillMaxWidth().height(ui.keysHeightDp.dp).verticalScroll(rememberScrollState()).padding(horizontal = 8.dp, vertical = 6.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        if (shots.isNotEmpty()) {
            Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                shots.forEach { (uri, bmp) ->
                    androidx.compose.foundation.Image(
                        bmp,
                        "Screenshot",
                        contentScale = androidx.compose.ui.layout.ContentScale.Crop,
                        modifier = Modifier.height(96.dp).width(54.dp).clip(RoundedCornerShape(8.dp))
                            .background(Colors.chip)
                            .let { m -> if (sent == uri) m.graphicsLayer { alpha = 0.5f } else m }
                            .combinedClickable(
                                onClick = {
                                    // Into the app as an image; where the field takes none, onto the clipboard to paste.
                                    if (typist.sendShot(uri)) typist.showKeys() else sent = uri
                                },
                                onLongClick = { typist.holdBuzz(); ui.bigPicture = uri },
                            ),
                    )
                }
            }
            if (sent != null) Text("Copied", color = Colors.muted, fontSize = 12.sp)
        }
        // Pinned first, then the rest: short copies as chips filling each row, long
        // ones as two-line cards. Hold one: pin or delete it.
        var pins by remember { mutableStateOf(uk.ccez.keys.learn.Clips.pinned(ctx)) }
        var held by remember { mutableStateOf<String?>(null) }
        fun refresh() { items = uk.ccez.keys.learn.Clips.all(ctx); pins = uk.ccez.keys.learn.Clips.pinned(ctx) }
        @Composable
        fun Clip(t: String, pinned: Boolean, wide: Boolean) {
            val m = if (wide) Modifier.fillMaxWidth() else Modifier
            Row(
                m.clip(RoundedCornerShape(10.dp)).background(if (held == t) Color(0xFF3A3A3C) else Colors.chip)
                    .combinedClickable(
                        onClick = { if (held == t) held = null else { typist.paste(t); typist.showKeys() } },
                        onLongClick = { typist.holdBuzz(); held = t },
                    )
                    .padding(start = 12.dp, end = if (held == t) 4.dp else 12.dp, top = 9.dp, bottom = 9.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (pinned && held != t) { Icon(Icons.pin, null, tint = Colors.accent, modifier = Modifier.size(12.dp)); Spacer(Modifier.width(6.dp)) }
                Text(
                    t.replace('\n', ' '),
                    color = Colors.fg,
                    fontSize = 13.5.sp,
                    maxLines = if (wide) 2 else 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = if (wide) Modifier.weight(1f) else Modifier.widthIn(max = 220.dp),
                )
                if (held == t) {
                    Box(Modifier.width(34.dp).height(16.dp).clip(RoundedCornerShape(6.dp)).combinedClickable(onClick = {
                        uk.ccez.keys.learn.Clips.pin(ctx, t, !pinned); held = null; refresh()
                    }), contentAlignment = Alignment.Center) { Icon(Icons.pin, if (pinned) "Unpin" else "Pin", tint = if (pinned) Colors.accent else Colors.muted, modifier = Modifier.size(16.dp)) }
                    Box(Modifier.width(34.dp).height(16.dp).clip(RoundedCornerShape(6.dp)).combinedClickable(onClick = {
                        uk.ccez.keys.learn.Clips.remove(ctx, t); held = null; refresh()
                    }), contentAlignment = Alignment.Center) { Icon(Icons.trash, "Delete", tint = Colors.muted, modifier = Modifier.size(16.dp)) }
                }
            }
        }
        @Composable
        fun Group(list: List<String>, pinned: Boolean) {
            val (short, long) = list.partition { it.length <= 28 && '\n' !in it }
            if (short.isNotEmpty()) FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                short.forEach { Clip(it, pinned, wide = false) }
            }
            long.forEach { Clip(it, pinned, wide = true) }
        }
        if (pins.isNotEmpty()) {
            Group(pins, pinned = true)
            Box(Modifier.fillMaxWidth().padding(vertical = 2.dp).height(1.dp).background(Color(0xFF2A2A2C)))
        }
        Group(items.filter { it !in pins }, pinned = false)
    }
}

// --- settings ---

/** Every language the keyboard knows (data/languages.json): code → English name, own name, flag. */
object Langs {
    data class L(val code: String, val name: String, val own: String, val flag: String)
    private var all: List<L> = emptyList()

    fun all(ctx: Context): List<L> {
        if (all.isEmpty()) {
            val a = JSONObject(ctx.assets.open("languages.json").bufferedReader().readText()).getJSONArray("languages")
            all = (0 until a.length()).map { i -> a.getJSONArray(i).let { L(it.getString(0), it.getString(1), it.getString(2), it.getString(3)) } }
        }
        return all
    }

    fun of(ctx: Context, code: String) = all(ctx).firstOrNull { it.code == code }
}

/** Languages with a speech voice on the phone (installed, works offline). */
private object Voices {
    var ready by mutableStateOf<Set<String>?>(null)
    private var tts: TextToSpeech? = null

    fun load(ctx: Context) {
        if (ready != null || tts != null) return
        tts = TextToSpeech(ctx.applicationContext) { status ->
            val t = tts
            ready = if (status == TextToSpeech.SUCCESS && t != null) {
                runCatching {
                    t.voices.orEmpty()
                        .filter { !it.isNetworkConnectionRequired && TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED !in it.features }
                        .map { v -> v.locale.language.let { OLD[it] ?: it } }
                        .toSet()
                }.getOrDefault(emptySet())
            } else {
                emptySet()
            }
            t?.shutdown()
            tts = null
        }
    }

    private val OLD = mapOf("iw" to "he", "in" to "id", "nb" to "no", "nn" to "no")
}

private enum class Page { MAIN, ADD, DEFAULT, LICENCES }

@Composable
private fun SettingsPanel(ui: UiState, typist: Typist, version: Int) {
    var page by remember { mutableStateOf(Page.MAIN) }
    val scroll = rememberScrollState()
    LaunchedEffect(page) { scroll.scrollTo(0) }
    Column(
        Modifier.fillMaxWidth().height(ui.keysHeightDp.dp).verticalScroll(scroll).padding(horizontal = 12.dp, vertical = 4.dp),
    ) {
        when (page) {
            Page.MAIN -> SettingsMain(version) { page = it }
            Page.ADD -> LanguageList("Add", version, { page = Page.MAIN }) { code ->
                set("languages.learning", JSONArray(learning() + code).toString())
                page = Page.MAIN
            }
            Page.DEFAULT -> LanguageList("Default language", version, { page = Page.MAIN }, includeAll = true) { code ->
                set("languages.default", "\"$code\"")
                page = Page.MAIN
            }
            Page.LICENCES -> Licences { page = Page.MAIN }
        }
        Spacer(Modifier.height(8.dp))
    }
}

private fun learning() = App.instance.engine.settingStrings("languages.learning")

private fun set(key: String, json: String) {
    App.instance.engine.setSetting(key, json)
    App.instance.settingsChanged()
}

@Composable
private fun SettingsMain(version: Int, go: (Page) -> Unit) {
    val ctx = LocalContext.current
    val e = App.instance.engine
    Heading("Buttons")
    TwoColumns(
        listOf(
            Triple(Icons.clip, "Clipboard", "buttons.clipboard"),
            Triple(Icons.face, "Face key", "buttons.face"),
            Triple(Icons.mic, "Dictation", "buttons.mic"),
            Triple(Icons.cam, "Camera text", "buttons.camera"),
            Triple(Icons.tr, "Translate", "learning.strip"),
            // A word tapped in the strip, copied (owner, 2026-11-12).
            Triple(Icons.copy, "Copy words", "learning.copy_click"),
            Triple(Icons.vibrate, "Vibrate", "typing.vibrate"),
        ),
        version,
    )
    Heading("Typing")
    TwoColumns(
        listOf(
            Triple(Icons.check, "Autocorrect", "typing.autocorrect"),
            Triple(Icons.capital, "Capitals", "typing.capitals"),
            Triple(Icons.dot, "Double space .", "typing.double_space_period"),
            Triple(Icons.sound, "Sound", "typing.sound"),
            Triple(Icons.number, "Numbers", "typing.numbers"),
            Triple(Icons.swipe, "Swipe", "typing.swipe"),
            Triple(Icons.capital, "Telex (Tiếng Việt)", "typing.telex"),
            Triple(Icons.size, "Size", ""),
        ),
        version,
    )
    Heading("Languages")
    FlagGrid(learning(), version, go)
    Heading("Default language")
    val def = remember(version) { App.instance.text("languages.default") }
    val d = Langs.of(ctx, def)
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).combinedClickableCompat { go(Page.DEFAULT) }.padding(vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(d?.flag ?: def, fontSize = 22.sp)
        Spacer(Modifier.weight(1f))
        Text("›", color = Colors.muted, fontSize = 20.sp)
    }
    Heading("AI")
    AiSettings(version)
    Heading("About")
    if (uk.ccez.keys.BuildConfig.STORE && uk.ccez.keys.BuildConfig.DONATE_URL.isNotEmpty()) {
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).combinedClickableCompat {
                ctx.startActivity(android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(uk.ccez.keys.BuildConfig.DONATE_URL)).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK))
            }.padding(vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Donate", color = Colors.fg, fontSize = 14.sp, modifier = Modifier.weight(1f))
            Text("›", color = Colors.muted, fontSize = 20.sp)
        }
    }
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).combinedClickableCompat { go(Page.LICENCES) }.padding(vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("Licences", color = Colors.fg, fontSize = 14.sp, modifier = Modifier.weight(1f))
        Text("›", color = Colors.muted, fontSize = 20.sp)
    }
}

@Composable
private fun AiSettings(version: Int) {
    val ctx = LocalContext.current
    val e = App.instance.engine
    val provider = remember(version) { e.setting("ai.provider").trim('"') }
    var hint by remember(version) { mutableStateOf(uk.ccez.keys.learn.Secrets.hint(ctx)) }
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        listOf("muse" to "Muse", "deepseek" to "DeepSeek", "zen" to "Zen").forEach { (id, label) ->
            Text(
                label,
                color = if (id == provider) Colors.accent else Colors.fg,
                fontSize = 12.5.sp,
                modifier = Modifier.clip(RoundedCornerShape(14.dp)).background(if (id == provider) Colors.faceOn else Colors.chip)
                    .combinedClickableCompat { set("ai.provider", "\"$id\"") }.padding(horizontal = 12.dp, vertical = 6.dp),
            )
        }
    }
    Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(hint ?: "Key", color = if (hint == null) Colors.dim else Colors.fg, fontSize = 13.5.sp, modifier = Modifier.weight(1f))
        // The key from the clipboard: copied wherever it lives, pasted here.
        Action("Paste") {
            val clip = ctx.getSystemService(android.content.ClipboardManager::class.java)?.primaryClip
            val text = clip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(ctx)?.toString()?.trim()
            if (!text.isNullOrEmpty() && !text.contains(' ')) {
                uk.ccez.keys.learn.Secrets.set(ctx, text)
                hint = uk.ccez.keys.learn.Secrets.hint(ctx)
                // A key never stays in the clipboard history or the chip.
                uk.ccez.keys.learn.Clips.remove(ctx, text)
                App.instance.clearClipChip?.invoke()
            }
        }
        if (hint != null) Action("Remove") { uk.ccez.keys.learn.Secrets.set(ctx, null); hint = null }
    }
    // Apps given the key (keyring/), or refused it: Remove makes one ask again.
    var apps by remember(version) { mutableStateOf(uk.ccez.keys.keyring.Keyring.decided(ctx)) }
    apps.forEach { (pkg, ok) ->
        Row(Modifier.fillMaxWidth().padding(top = 6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(uk.ccez.keys.keyring.Keyring.label(ctx, pkg), color = if (ok) Colors.fg else Colors.dim, fontSize = 13.5.sp, modifier = Modifier.weight(1f))
            if (!ok) Text("Refused", color = Colors.dim, fontSize = 12.sp)
            Action("Remove") { uk.ccez.keys.keyring.Keyring.forget(ctx, pkg); apps = uk.ccez.keys.keyring.Keyring.decided(ctx) }
        }
    }
    TwoColumns(listOf(Triple(Icons.spark, "Corrections", "ai.tips")), version)
}

/** The learning order as numbered flags, five a row, + last; tap a flag to move or remove it. */
@Composable
private fun FlagGrid(order: List<String>, @Suppress("UNUSED_PARAMETER") version: Int, go: (Page) -> Unit) {
    val ctx = LocalContext.current
    var picked by remember { mutableStateOf<String?>(null) }
    val tiles = order.map { it as String? } + listOf(null)
    Column(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        tiles.chunked(5).forEach { row ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                row.forEach { code ->
                    val i = if (code == null) -1 else order.indexOf(code)
                    Box(
                        Modifier.weight(1f).height(52.dp).clip(RoundedCornerShape(12.dp))
                            .background(if (code != null && code == picked) Colors.popupSel else Colors.key)
                            .combinedClickableCompat { if (code == null) go(Page.ADD) else picked = if (picked == code) null else code },
                        contentAlignment = Alignment.Center,
                    ) {
                        if (code == null) {
                            Text("+", color = Colors.accent, fontSize = 22.sp)
                        } else {
                            Text(Langs.of(ctx, code)?.flag ?: code, fontSize = 26.sp)
                            Text(
                                "${i + 1}",
                                color = Colors.muted,
                                fontSize = 10.sp,
                                fontWeight = FontWeight.Bold,
                                modifier = Modifier.align(Alignment.BottomEnd).padding(end = 5.dp, bottom = 2.dp),
                            )
                        }
                    }
                }
                repeat(5 - row.size) { Spacer(Modifier.weight(1f)) }
            }
        }
        val p = picked
        if (p != null && p in order) {
            val i = order.indexOf(p)
            fun move(to: Int) {
                val l = order.toMutableList()
                l.add(to.coerceIn(0, l.lastIndex), l.removeAt(i))
                set("languages.learning", JSONArray(l).toString())
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(Langs.of(ctx, p)?.name ?: p, color = Colors.fg, fontSize = 13.sp, modifier = Modifier.weight(1f))
                if (i > 0) Action("First") { move(0) }
                if (i > 0) Action("Earlier") { move(i - 1) }
                if (i < order.lastIndex) Action("Later") { move(i + 1) }
                Action("Remove") { set("languages.learning", JSONArray(order - p).toString()); picked = null }
            }
        }
    }
}

@Composable
private fun Action(label: String, onTap: () -> Unit) = Text(
    label,
    color = Colors.fg,
    fontSize = 12.5.sp,
    modifier = Modifier.clip(RoundedCornerShape(14.dp)).background(Colors.chip).combinedClickableCompat(onTap).padding(horizontal = 10.dp, vertical = 6.dp),
)

/** Languages to choose from, by name, with 🔊 where the phone has a voice for it. */
@Composable
private fun LanguageList(title: String, @Suppress("UNUSED_PARAMETER") version: Int, back: () -> Unit, includeAll: Boolean = false, pick: (String) -> Unit) {
    val ctx = LocalContext.current
    LaunchedEffect(Unit) { Voices.load(ctx) }
    val voices = Voices.ready
    BackRow(title, back)
    val skip = if (includeAll) emptySet() else learning().toSet()
    Langs.all(ctx).filter { it.code !in skip }.forEach { l ->
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).combinedClickableCompat { pick(l.code) }.padding(vertical = 9.dp, horizontal = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(l.flag, fontSize = 20.sp)
            Spacer(Modifier.width(12.dp))
            Text(l.name, color = Colors.fg, fontSize = 14.sp)
            if (l.own != l.name) {
                Spacer(Modifier.width(8.dp))
                Text(l.own, color = Colors.dim, fontSize = 13.sp, maxLines = 1)
            }
            Spacer(Modifier.weight(1f))
            if (voices != null && l.code in voices) Icon(Icons.sound, "Voice", tint = Colors.muted, modifier = Modifier.size(18.dp))
        }
    }
}

@Composable
private fun BackRow(title: String, back: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).combinedClickableCompat(back).padding(vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("‹", color = Colors.fg, fontSize = 22.sp, modifier = Modifier.padding(end = 12.dp))
        Text(title, color = Colors.fg, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun Licences(back: () -> Unit) {
    BackRow("Licences", back)
    listOf(
        Triple("ccez-keys", "The keyboard", "MIT"),
        Triple("wordfreq", "Word lists · Robyn Speer", "CC BY-SA 4.0"),
        Triple("Wiktionary", "German capitals · via kaikki.org", "CC BY-SA 4.0"),
        Triple("UniFFI", "Rust ↔ Kotlin · Mozilla", "MPL 2.0"),
        Triple("JNA", "Native calls", "Apache 2.0"),
        Triple("Jetpack Compose", "Panels · Google", "Apache 2.0"),
    ).forEach { (name, what, licence) ->
        Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(name, color = Colors.fg, fontSize = 14.sp)
                Text(what, color = Colors.dim, fontSize = 12.sp)
            }
            Text(licence, color = Colors.muted, fontSize = 12.sp)
        }
        Box(Modifier.fillMaxWidth().height(1.dp).background(Colors.line))
    }
}

@Composable
private fun Heading(t: String) = Text(
    t.uppercase(),
    color = Colors.dim,
    fontSize = 11.sp,
    letterSpacing = 0.8.sp,
    modifier = Modifier.padding(top = 14.dp, bottom = 4.dp),
)

/**
 * Two settings a row; the whole cell is the switch. `version` makes it
 * redraw when a setting changes (the values come from the core, which
 * Compose can't watch).
 */
@Composable
private fun TwoColumns(rows: List<Triple<ImageVector, String, String>>, version: Int) {
    val app = App.instance
    Column {
        rows.chunked(2).forEach { pair ->
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                pair.forEach { (icon, label, key) ->
                    val on = remember(version, key) { key.isNotEmpty() && app.flag(key) }
                    Row(
                        Modifier.weight(1f).height(44.dp).clip(RoundedCornerShape(10.dp))
                            .let { m -> if (key.isEmpty()) m else m.combinedClickableCompat { set(key, (!on).toString()) } }
                            .padding(horizontal = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(icon, null, tint = Colors.muted, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(10.dp))
                        Text(label, color = Colors.fg, fontSize = 13.5.sp, modifier = Modifier.weight(1f), maxLines = 1)
                        if (key.isEmpty()) SizeStepper(version) else Switch(on)
                    }
                }
                if (pair.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun Switch(on: Boolean) {
    val x by androidx.compose.animation.core.animateDpAsState(if (on) 14.dp else 0.dp, tween(120), label = "switch")
    Box(Modifier.width(32.dp).height(18.dp).clip(RoundedCornerShape(10.dp)).background(if (on) Colors.switchOn else Colors.switchOff)) {
        Box(Modifier.padding(2.dp).offset(x = x).size(14.dp).clip(RoundedCornerShape(7.dp)).background(if (on) Colors.accent else Color(0xFF8A8A8A)))
    }
}

@Composable
private fun SizeStepper(version: Int) {
    val now = remember(version) { App.instance.engine.setting("keyboard.height").toIntOrNull() ?: 100 }
    fun to(v: Int) = set("keyboard.height", v.coerceIn(80, 130).toString())
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("−", color = Colors.fg, fontSize = 18.sp, modifier = Modifier.clip(RoundedCornerShape(8.dp)).combinedClickableCompat { to(now - 5) }.padding(horizontal = 10.dp, vertical = 4.dp))
        Text("$now", color = Colors.muted, fontSize = 12.sp)
        Text("+", color = Colors.fg, fontSize = 18.sp, modifier = Modifier.clip(RoundedCornerShape(8.dp)).combinedClickableCompat { to(now + 5) }.padding(horizontal = 10.dp, vertical = 4.dp))
    }
}

// --- icons (stroked, 24-unit paths) ---

internal object Icons {
    private fun stroke(vararg paths: String) = ImageVector.Builder(defaultWidth = 24.dp, defaultHeight = 24.dp, viewportWidth = 24f, viewportHeight = 24f).apply {
        paths.forEach {
            addPath(
                addPathNodes(it),
                stroke = SolidColor(Color.White),
                strokeLineWidth = 1.7f,
                strokeLineCap = StrokeCap.Round,
                strokeLineJoin = StrokeJoin.Round,
            )
        }
    }.build()

    val tools = stroke("M4 4h6v6H4z", "M14 4h6v6h-6z", "M4 14h6v6H4z", "M14 14h6v6h-6z")
    val pin = stroke("M9 4h6l-1 5 3 3H7l3-3z", "M12 12v8")
    val trash = stroke("M5 7h14", "M10 7V5h4v2", "M7 7l1 12h8l1-12")
    val clip = stroke("M7 4h10a2 2 0 0 1 2 2v13a2 2 0 0 1 -2 2h-10a2 2 0 0 1 -2 -2v-13a2 2 0 0 1 2 -2z", "M9 4V3h6v1", "M9 10h6M9 14h6")
    val gear = stroke(
        "M15 12a3 3 0 1 1 -6 0a3 3 0 1 1 6 0z",
        "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z",
    )
    val face = stroke("M21 12a9 9 0 1 1 -18 0a9 9 0 1 1 18 0z", "M8.5 14.5a4.5 4.5 0 0 0 7 0", "M9 9.5h.01M15 9.5h.01")
    val check = stroke("M4 12l5 5L20 6")
    val capital = stroke("M5 19l6-14 6 14", "M7.5 13.5h7", "M19 9V5M17 7l2-2 2 2")
    val dot = stroke("M4 17h3M10 17h3", "M18 17h.01")
    val vibrate = stroke("M10 4h4a1.5 1.5 0 0 1 1.5 1.5v13a1.5 1.5 0 0 1 -1.5 1.5h-4a1.5 1.5 0 0 1 -1.5 -1.5v-13a1.5 1.5 0 0 1 1.5 -1.5z", "M4 9v6M20 9v6")
    val sound = stroke("M4 9h4l5-4v14l-5-4H4z", "M16.5 8.5a5 5 0 0 1 0 7")
    val cam = stroke("M4 8h3l1.5-2h7L17 8h3v11H4z", "M15.5 13a3.5 3.5 0 1 1 -7 0a3.5 3.5 0 1 1 7 0z")
    val mic = stroke("M9 6a3 3 0 0 1 6 0v5a3 3 0 0 1 -6 0z", "M5 11a7 7 0 0 0 14 0M12 18v3")
    val tr = stroke("M3 5h9M7.5 3v2M5 5c.5 3 2.5 5.5 5.5 7M10 5c-.7 3.5-3 6-6 7.5", "M13 20l4-10 4 10M14.5 16.5h5")
    val spark = stroke("M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z", "M19 16l.7 1.8 1.8.7-1.8.7L19 21l-.7-1.8-1.8-.7 1.8-.7z")
    val book = stroke("M4 5.5A2.5 2.5 0 0 1 6.5 3H20v15H6.5A2.5 2.5 0 0 0 4 20.5z", "M4 20.5A2.5 2.5 0 0 0 6.5 23H20v-5")
    val swipe = stroke("M3 15c3-6 5-8 7-5s3 4 5-1 4-3 6 0")
    val number = stroke("M5 9l2-1.5V16", "M10.5 9.5a2 2 0 0 1 4 .5c0 2-4 3.5-4 6h4", "M17.5 8h3l-2 3a2.5 2.5 0 1 1-1.5 4.5")
    val copy = stroke("M9 9h10a1 1 0 0 1 1 1v10a1 1 0 0 1 -1 1h-10a1 1 0 0 1 -1 -1v-10a1 1 0 0 1 1 -1z", "M5 15a1 1 0 0 1 -1 -1v-10a1 1 0 0 1 1 -1h10a1 1 0 0 1 1 1v1")
    val size = stroke("M4 9h16M4 15h16", "M12 3v4M12 17v4")
}
