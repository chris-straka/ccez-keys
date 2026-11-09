package uk.ccez.keys.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.animation.togetherWith
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.animation.core.animateFloat
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.asComposeRenderEffect
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uk.ccez.keys.Colors
import uk.ccez.keys.Typist
import uk.ccez.keys.UiState
import uk.ccez.keys.core.StripWord
import uk.ccez.keys.core.WordStatus
import uk.ccez.keys.learn.CardView
import uk.ccez.keys.learn.Speaker
import uk.ccez.keys.learn.StripView

private val RTL = setOf("ar", "fa", "he", "ur")
private val STRIP_BG = Color(0xFF181818)

/**
 * The learning strip: flags on the left (tap: next language; hold: put
 * this translation in instead of what was typed), the words with their
 * readings (tap: card), 🔊 on the right.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun StripRow(s: StripView, typist: Typist) {
    val ctx = LocalContext.current
    var drag by remember { mutableFloatStateOf(0f) }
    Row(
        Modifier.fillMaxSize().background(STRIP_BG)
            .pointerInput(s.lang) {
                detectHorizontalDragGestures(
                    // Right: the language before on the owner's list; left: the one after.
                    // (The words don't scroll by hand, so a swipe anywhere on the line works.)
                    onDragEnd = { if (drag > 60f) typist.strip.step(-1) else if (drag < -60f) typist.strip.step(1); drag = 0f },
                    onDragCancel = { drag = 0f },
                ) { _, d -> drag += d }
            }
            .padding(start = 6.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // A new language slides in from the side swiped towards; the AI's wording
        // fades in over ML Kit's. Typing updates the line in place.
        androidx.compose.animation.AnimatedContent(
            targetState = s,
            contentKey = { it.lang to it.refined },
            transitionSpec = {
                val ease = androidx.compose.animation.core.FastOutSlowInEasing
                if (initialState.lang != targetState.lang) {
                    val dir = targetState.moved
                    (androidx.compose.animation.slideInHorizontally(androidx.compose.animation.core.tween(170, easing = ease)) { w -> dir * w / 5 } +
                        androidx.compose.animation.fadeIn(androidx.compose.animation.core.tween(150, delayMillis = 20)))
                        .togetherWith(
                            androidx.compose.animation.slideOutHorizontally(androidx.compose.animation.core.tween(170, easing = ease)) { w -> -dir * w / 5 } +
                                androidx.compose.animation.fadeOut(androidx.compose.animation.core.tween(110)),
                        )
                } else {
                    androidx.compose.animation.fadeIn(androidx.compose.animation.core.tween(320, easing = ease))
                        .togetherWith(androidx.compose.animation.fadeOut(androidx.compose.animation.core.tween(200)))
                }.using(androidx.compose.animation.SizeTransform(clip = true))
            },
            modifier = Modifier.weight(1f),
            label = "strip",
        ) { v -> Row(verticalAlignment = Alignment.CenterVertically) { StripLine(v, typist) } }
        // The AI at work (a slow violet blink) or its wording in place (✦), in a
        // slot always there: appearing, they pushed the words (owner, 2026-11-08).
        Box(Modifier.size(30.dp).clip(RoundedCornerShape(15.dp)).let { m ->
            // Tap: ask Muse about this wording. Hold: this app's wording polite or casual.
            if (s.refined) m.combinedClickable(onClick = { typist.openAsk("${s.source} → ${s.text}") }, onLongClick = { typist.flipRegister() }) else m
        }, contentAlignment = Alignment.Center) {
            if (s.pending) {
                var bright by remember { mutableStateOf(true) }
                androidx.compose.runtime.LaunchedEffect(Unit) { while (true) { kotlinx.coroutines.delay(550); bright = !bright } }
                Box(Modifier.size(6.dp).graphicsLayer { alpha = if (bright) 1f else 0.3f }.clip(RoundedCornerShape(3.dp)).background(VIOLET))
            } else if (s.refined) {
                Icon(Icons.spark, "Ask Muse", tint = if (typist.polite()) VIOLET else VIOLET.copy(alpha = 0.6f), modifier = Modifier.size(13.dp))
            }
        }
        // No speaker here: the cards have one (owner, 2026-11-08).
    }
}

/** A flag, faded and greyed (owner, 2026-11-08: a bright one drew the eye). */
@Composable
internal fun MutedFlag(flag: String, size: Int) {
    Text(
        flag,
        fontSize = size.sp,
        modifier = Modifier.graphicsLayer {
            alpha = 0.55f
            if (android.os.Build.VERSION.SDK_INT >= 31) {
                renderEffect = android.graphics.RenderEffect.createColorFilterEffect(
                    android.graphics.ColorMatrixColorFilter(android.graphics.ColorMatrix().apply { setSaturation(0.25f) }),
                ).asComposeRenderEffect()
            }
        },
    )
}

/** Its children in a row, the first always, the rest while they fit whole. */
@Composable
private fun FitRow(modifier: Modifier, shift: Float = 0f, content: @Composable () -> Unit) {
    androidx.compose.ui.layout.Layout(content, modifier) { measurables, constraints ->
        val loose = constraints.copy(minWidth = 0, maxWidth = androidx.compose.ui.unit.Constraints.Infinity)
        val placeables = measurables.map { it.measure(loose) }
        val shown = mutableListOf<androidx.compose.ui.layout.Placeable>()
        var used = 0
        for ((i, p) in placeables.withIndex()) {
            if (i > 0 && used + p.width > constraints.maxWidth) break
            shown += p
            used += p.width
        }
        val h = shown.maxOfOrNull { it.height } ?: 0
        layout(constraints.maxWidth, h.coerceIn(constraints.minHeight, constraints.maxHeight)) {
            // A first one longer than the line: its end shows (the words being typed),
            // unless held and dragged back.
            val far = minOf(0, constraints.maxWidth - shown.first().width)
            var x = (far + shift.toInt()).coerceIn(far, 0)
            shown.forEach { p -> p.placeRelative(x, (h - p.height) / 2); x += p.width }
        }
    }
}

/** The flag and the words: what slides when the language changes. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun androidx.compose.foundation.layout.RowScope.StripLine(s: StripView, typist: Typist) {
    val more = if (s.lang in RTL) emptyList() else s.more
    // Room over the words for furigana or pinyin, only when a word has some;
    // flags and bars move down the same, so all sit level.
    // Furigana and pinyin are on the card, not over the line (owner, 2026-11-08: a shorter line).
    val readings = false
    val level = Modifier.padding(top = if (readings) 12.dp else 0.dp)
    // Every language the same: a small dim flag, then its words (owner, 2026-11-08).
    // Hold the first flag: this translation in place of what was typed. No tap
    // (swipes change language).
    Spacer(Modifier.width(4.dp))
    Box(
        level.clip(RoundedCornerShape(4.dp)).combinedClickable(onClick = {}, onLongClick = { typist.replaceSentence(s.text, of = s.source) }),
        contentAlignment = Alignment.Center,
    ) { MutedFlag(s.flag, 12) }
    Spacer(Modifier.width(5.dp))
    if (more.isNotEmpty()) {
        // The same in the next languages, "chat | Katze | gato", as many as fit;
        // each word taps to its card, in its language.
        var shift by remember(s.text) { mutableFloatStateOf(0f) }
        FitRow(
            Modifier.weight(1f).clipToBounds().pointerInput(s.text) {
                detectDragGesturesAfterLongPress(onDragStart = { typist.holdBuzz() }) { change, d ->
                    change.consume()
                    shift += d.x
                }
            },
            shift,
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) { s.words.forEach { w -> Word(w, s.lang, typist, readings) } }
            more.forEach { seg ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    // Space, not a bar: the flag marks where a language starts.
                    Spacer(Modifier.width(16.dp))
                    Box(level) { MutedFlag(seg.flag, 12) }
                    Spacer(Modifier.width(5.dp))
                    seg.words.forEach { w -> Word(w, seg.lang, typist, readings) }
                }
            }
        }
        return
    }
    // One line, kept scrolled to its end: the words being typed now show.
    val scroll = androidx.compose.foundation.rememberScrollState()
    androidx.compose.runtime.LaunchedEffect(s.text) { scroll.scrollTo(scroll.maxValue) }
    CompositionLocalProvider(LocalLayoutDirection provides if (s.lang in RTL) LayoutDirection.Rtl else LayoutDirection.Ltr) {
        Row(
            Modifier.weight(1f).horizontalScroll(scroll, enabled = false).pointerInput(s.text) {
                // Hold, then drag: the line moves (a plain swipe changes language).
                detectDragGesturesAfterLongPress(onDragStart = { typist.holdBuzz() }) { change, d ->
                    change.consume()
                    scroll.dispatchRawDelta(-d.x)
                }
            },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            s.words.forEach { w -> Word(w, s.lang, typist, readings) }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun Word(w: StripWord, lang: String, typist: Typist, readings: Boolean) {
    if (!w.word && w.text.isBlank()) {
        Spacer(Modifier.width(4.dp))
        return
    }
    val color = when {
        !w.word -> Colors.fg
        w.status == WordStatus.KNOWN -> Colors.dim
        else -> Colors.fg
    }
    val underline = w.word && w.status == WordStatus.LEARNING
    val accent = Colors.accent
    Row(
        Modifier.let { m ->
            if (!w.word) m else m.clip(RoundedCornerShape(4.dp)).combinedClickable(
                // Tap: the card (Learning / Known), and the word said. A hold drags
                // the line instead (owner, 2026-11-08: to see a long sentence's start).
                onClick = { typist.strip.open(lang, w) },
            )
        },
    ) {
        w.ruby.forEach { r ->
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                // The same room over every word, so lines stay level.
                if (readings) Text(r.rt, fontSize = 9.5.sp, lineHeight = 11.sp, color = if (w.status == WordStatus.KNOWN) Color(0xFF555555) else Colors.muted, modifier = Modifier.height(12.dp))
                Text(
                    r.text,
                    fontSize = 15.5.sp,
                    color = color,
                    modifier = if (!underline) Modifier else Modifier.drawBehind {
                        val y = size.height - 1.dp.toPx()
                        drawLine(accent, Offset(0f, y), Offset(size.width, y), strokeWidth = 1.5.dp.toPx())
                    },
                )
            }
        }
    }
}

/**
 * A tapped word: reading over it for Japanese and Chinese, its meaning
 * under, parts of speech as small tags, Learning and Known (the one
 * standard across the apps). Covers the keys; a tap outside closes it.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun BoxScope.CardOverlay(c: CardView, ui: UiState, typist: Typist) {
    val ctx = LocalContext.current
    val status = c.card?.status ?: typist.engineStatus(c.lang, c.lemma.ifEmpty { c.word })
    Box(
        Modifier.matchParentSize().background(Color(0xCC0B0B0C)).combinedClickable(onClick = { ui.card = null }),
        contentAlignment = Alignment.TopCenter,
    ) {
        Column(
            Modifier.padding(horizontal = 8.dp, vertical = 8.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(Color(0xFF1D1D1F))
                .combinedClickable(onClick = {}).padding(horizontal = 14.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            val card = c.card
            val head = card?.lemma?.ifEmpty { null } ?: c.lemma.ifEmpty { c.word }
            Row(verticalAlignment = Alignment.Bottom) {
                Column {
                    val reading = card?.reading?.ifEmpty { null } ?: c.reading
                    if (reading.isNotEmpty() && reading != head) Text(reading, color = Colors.muted, fontSize = 12.sp)
                    Row(verticalAlignment = Alignment.Bottom) {
                        val article = c.article ?: card?.article.orEmpty()
                        // On the same baseline as the word ("la prueba").
                        if (article.isNotEmpty()) Text(if (article.endsWith("'")) article else "$article ", color = Colors.dim, fontSize = 20.sp, modifier = Modifier.alignByBaseline())
                        Text(head, color = Colors.fg, fontSize = 26.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.alignByBaseline())
                    }
                }
                Box(
                    Modifier.padding(start = 8.dp, bottom = 2.dp).size(32.dp).clip(RoundedCornerShape(16.dp)).combinedClickable(onClick = { Speaker.say(ctx, head, c.lang) }),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.sound, "Listen", tint = Colors.muted, modifier = Modifier.size(18.dp)) }
                Spacer(Modifier.weight(1f))
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp), modifier = Modifier.align(Alignment.Top)) {
                    card?.pos.orEmpty().take(2).forEach { p ->
                        Text(
                            p,
                            color = Colors.muted,
                            fontSize = 10.sp,
                            modifier = Modifier.clip(RoundedCornerShape(4.dp)).background(Color(0xFF2A2A2C)).padding(horizontal = 5.dp, vertical = 1.dp),
                        )
                    }
                }
            }
            // What it means here: Muse's reading of it in this sentence (✦), else the
            // dictionary's best match, Muse's answer fading in over it.
            var more by remember(c.word, c.lang) { mutableStateOf(false) }
            val meanings = card?.meanings.orEmpty().ifEmpty { listOfNotNull(c.meaning) }
            androidx.compose.animation.Crossfade(c.here, animationSpec = androidx.compose.animation.core.tween(220), label = "here") { here ->
                Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    if (here != null) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(Icons.spark, "Muse", tint = VIOLET, modifier = Modifier.size(13.dp))
                            Spacer(Modifier.width(6.dp))
                            Text(here, color = Colors.fg, fontSize = 17.sp, maxLines = 2)
                        }
                        c.note?.let { Text(it, color = Colors.muted, fontSize = 13.sp, maxLines = 3) }
                    } else {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            meanings.firstOrNull()?.let { Text(it, color = Colors.fg, fontSize = 17.sp, maxLines = 2, modifier = Modifier.weight(1f, fill = false)) }
                            if (c.asking) {
                                var bright by remember { mutableStateOf(true) }
                                androidx.compose.runtime.LaunchedEffect(Unit) { while (true) { kotlinx.coroutines.delay(550); bright = !bright } }
                                Box(Modifier.padding(start = 8.dp).size(6.dp).graphicsLayer { alpha = if (bright) 1f else 0.3f }.clip(RoundedCornerShape(3.dp)).background(VIOLET))
                            }
                        }
                    }
                }
            }
            // 📚: the dictionary's other meanings.
            val rest = if (c.here != null) meanings else meanings.drop(1)
            if (more && rest.isNotEmpty()) Text(rest.joinToString("; "), color = Colors.muted, fontSize = 14.sp, maxLines = 5)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val key = card?.lemma?.ifEmpty { null } ?: c.lemma.ifEmpty { c.word }
                StatusButton("Learning", status == WordStatus.LEARNING) {
                    typist.setStatus(c.lang, key, if (status == WordStatus.LEARNING) WordStatus.NEW else WordStatus.LEARNING)
                    ui.card = null
                }
                StatusButton("Known", status == WordStatus.KNOWN) {
                    typist.setStatus(c.lang, key, if (status == WordStatus.KNOWN) WordStatus.NEW else WordStatus.KNOWN)
                    ui.card = null
                }
                if (rest.isNotEmpty()) Box(
                    Modifier.width(44.dp).height(36.dp).clip(RoundedCornerShape(8.dp)).background(if (more) Color(0xFF3A3A3C) else Color(0xFF2A2A2C))
                        .combinedClickable(onClick = { more = !more }),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.book, "Dictionary", tint = Colors.muted, modifier = Modifier.size(17.dp)) }
                Box(
                    Modifier.width(44.dp).height(36.dp).clip(RoundedCornerShape(8.dp)).background(VIOLET_BG)
                        .combinedClickable(onClick = { typist.openAsk("$key (${ui.strip?.text ?: c.word})") }),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.spark, "Ask", tint = VIOLET, modifier = Modifier.size(17.dp)) }
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun androidx.compose.foundation.layout.RowScope.StatusButton(label: String, on: Boolean, onTap: () -> Unit) {
    Box(
        Modifier.weight(1f).height(36.dp).clip(RoundedCornerShape(8.dp))
            .background(if (on) Color(0xFF3A2422) else Color(0xFF2A2A2C)).combinedClickable(onClick = onTap),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = if (on) Colors.accent else Colors.fg, fontSize = 13.sp) }
}

private val VIOLET = Color(0xFFB9A4FF)
private val VIOLET_BG = Color(0xFF221B38)

/** A correction, as ccez-llm shows one: crossed out what goes, green what comes in; why under it. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun TipNote(t: uk.ccez.keys.learn.Tip, typist: Typist) {
    Row(
        Modifier.fillMaxSize().background(VIOLET_BG).padding(start = 12.dp, end = 8.dp, top = 5.dp, bottom = 5.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.spark, null, tint = VIOLET, modifier = Modifier.size(18.dp).combinedClickable(onClick = { typist.openAsk(t.sentence) }))
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            val text = androidx.compose.ui.text.buildAnnotatedString {
                t.fix.parts.forEach { p ->
                    when (p.kind) {
                        uk.ccez.keys.core.FixKind.SAME -> append(p.text)
                        uk.ccez.keys.core.FixKind.DEL -> withStyle(androidx.compose.ui.text.SpanStyle(color = Color(0xFFE09A9A), textDecoration = androidx.compose.ui.text.style.TextDecoration.LineThrough)) { append(p.text) }
                        uk.ccez.keys.core.FixKind.INS -> withStyle(androidx.compose.ui.text.SpanStyle(color = Color(0xFF9BE7B0), background = Color(0x2463D471))) { append(p.text) }
                    }
                }
            }
            Text(text, color = Color(0xFFEFE9FF), fontSize = 14.sp, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
            if (t.fix.why.isNotEmpty()) Text(t.fix.why, color = Color(0xFFA79FC0), fontSize = 11.sp, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
        }
        Spacer(Modifier.width(8.dp))
        Text(
            "Fix",
            color = Color(0xFF1A1430),
            fontSize = 12.5.sp,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.clip(RoundedCornerShape(14.dp)).background(VIOLET).combinedClickable(onClick = { typist.applyTip() }).padding(horizontal = 12.dp, vertical = 5.dp),
        )
    }
}

/** Ask: the chat above the keys; the keys type into it until it's closed. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun AskPanel(a: uk.ccez.keys.learn.AskState, typist: Typist) {
    val ctx = LocalContext.current
    val scroll = androidx.compose.foundation.rememberScrollState()
    androidx.compose.runtime.LaunchedEffect(a.lines.size, a.lines.lastOrNull()?.text?.length) { scroll.animateScrollTo(scroll.maxValue) }
    Column(Modifier.fillMaxWidth().background(Color(0xFF141218)).padding(horizontal = 10.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.spark, null, tint = VIOLET, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(
                a.about,
                color = Color(0xFFCFC8E6),
                fontSize = 12.sp,
                maxLines = 1,
                modifier = Modifier.weight(1f).clip(RoundedCornerShape(10.dp)).background(Color(0xFF221F2B)).padding(horizontal = 9.dp, vertical = 3.dp),
            )
            // Carry on in ccez-llm.
            Box(
                Modifier.padding(start = 6.dp).size(30.dp).clip(RoundedCornerShape(15.dp)).combinedClickable(onClick = {
                    runCatching {
                        ctx.startActivity(
                            android.content.Intent(android.content.Intent.ACTION_SEND).setType("text/plain").setPackage("studio.ccez.app")
                                .putExtra(android.content.Intent.EXTRA_TEXT, typist.ask.transcript()).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK),
                        )
                    }
                }),
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.book, "ccez-llm", tint = Colors.muted, modifier = Modifier.size(18.dp)) }
            Box(
                Modifier.size(30.dp).clip(RoundedCornerShape(15.dp)).combinedClickable(onClick = { typist.closeAsk() }),
                contentAlignment = Alignment.Center,
            ) { Text("✕", color = Colors.muted, fontSize = 15.sp) }
        }
        Column(Modifier.fillMaxWidth().heightIn(max = 190.dp).verticalScroll(scroll), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            a.lines.forEach { l ->
                if (l.role == "user") {
                    Text(
                        l.text,
                        color = Colors.fg,
                        fontSize = 13.sp,
                        modifier = Modifier.align(Alignment.End).clip(RoundedCornerShape(14.dp, 14.dp, 4.dp, 14.dp)).background(Color(0xFF3B4C7A)).padding(horizontal = 11.dp, vertical = 6.dp),
                    )
                } else {
                    Text(
                        l.text.ifEmpty { "…" },
                        color = Color(0xFFE9E3FF),
                        fontSize = 13.sp,
                        lineHeight = 18.sp,
                        modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(VIOLET_BG).padding(horizontal = 11.dp, vertical = 8.dp),
                    )
                }
            }
        }
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(Color(0xFF1E1C24)).padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(a.input, color = Colors.fg, fontSize = 13.5.sp, maxLines = 3)
            Box(Modifier.width(1.5.dp).height(16.dp).background(Colors.accent))
            Spacer(Modifier.weight(1f))
            Text("↵", color = if (a.input.isBlank() || a.busy) Colors.dim else VIOLET, fontSize = 18.sp)
        }
    }
}

