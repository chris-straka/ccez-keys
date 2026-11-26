package uk.ccez.keys.ui

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.AlignmentLine
import androidx.compose.ui.layout.FirstBaseline
import androidx.compose.ui.layout.LastBaseline
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.Placeable
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uk.ccez.keys.core.Ruby
import kotlin.math.roundToInt

/**
 * Where each reading starts, in px, with the bases laid side by side from x = 0.
 * A reading is centred over its own base and pushed right so it never overlaps
 * the reading before it (with at least `gap` between them); a part with no
 * reading (width 0) gets the centre of its base and neither pushes nor is pushed.
 */
internal fun rubyStarts(baseWidths: List<Float>, rubyWidths: List<Float>, gap: Float): List<Float> {
    require(baseWidths.size == rubyWidths.size) { "baseWidths and rubyWidths must match" }
    val starts = ArrayList<Float>(baseWidths.size)
    var baseX = 0f
    var prevEnd = Float.NEGATIVE_INFINITY
    for (i in baseWidths.indices) {
        val bw = baseWidths[i]
        val rw = rubyWidths[i]
        val centered = baseX + (bw - rw) / 2f
        val x = if (rw > 0f) {
            val placed = maxOf(centered, prevEnd + gap)
            prevEnd = placed + rw
            placed
        } else {
            centered
        }
        starts += x
        baseX += bw
    }
    return starts
}

/**
 * A word with its readings placed over each kanji run: readings sit in a band
 * over the bases at the positions from [rubyStarts], shifted right so nothing
 * draws outside the layout. Reports [FirstBaseline] and [LastBaseline] on the
 * bases so `Modifier.alignBy(LastBaseline)` lines it up with neighbours.
 */
@Composable
fun RubyWord(
    parts: List<Ruby>,
    color: Color,
    fontSize: TextUnit,
    fontWeight: FontWeight?,
    rubyColor: Color,
    rubySize: TextUnit,
    modifier: Modifier = Modifier,
) {
    Layout(
        content = {
            for (p in parts) {
                Text(p.text, color = color, fontSize = fontSize, fontWeight = fontWeight, maxLines = 1)
            }
            for (p in parts) {
                if (p.rt.isNotEmpty()) {
                    Text(p.rt, color = rubyColor, fontSize = rubySize, lineHeight = 11.sp, maxLines = 1)
                }
            }
        },
        modifier = modifier,
    ) { measurables, constraints ->
        val loose = Constraints()
        val n = parts.size
        val bases = List(n) { i -> measurables[i].measure(loose) }
        val rubies = arrayOfNulls<Placeable>(n)
        var ri = n
        for (i in parts.indices) {
            if (parts[i].rt.isNotEmpty()) {
                rubies[i] = measurables[ri++].measure(loose)
            }
        }
        val baseWidths = bases.map { it.width.toFloat() }
        val rubyWidths = rubies.map { (it?.width ?: 0).toFloat() }
        val starts = rubyStarts(baseWidths, rubyWidths, 1.dp.toPx())

        val minRubyStart = rubies.indices.filter { rubies[it] != null }.minOfOrNull { starts[it] } ?: 0f
        val shift = -minOf(0f, minRubyStart)
        val shiftPx = shift.roundToInt()
        val basesTotal = bases.sumOf { it.width }
        val lastRubyEnd = rubies.indices.filter { rubies[it] != null }
            .maxOfOrNull { (shift + starts[it]).roundToInt() + rubies[it]!!.width } ?: 0
        val width = maxOf(shiftPx + basesTotal, lastRubyEnd)

        val bandH = rubies.filterNotNull().maxOfOrNull { it.height } ?: 0
        val baseH = bases.maxOfOrNull { it.height } ?: 0
        val height = bandH + baseH

        val firstBase = bases.map { it[FirstBaseline] }.filter { it != AlignmentLine.Unspecified }.maxOrNull()
        val lastBase = bases.map { it[LastBaseline] }.filter { it != AlignmentLine.Unspecified }.maxOrNull()
        val lines = buildMap<AlignmentLine, Int> {
            if (firstBase != null) put(FirstBaseline, bandH + firstBase)
            if (lastBase != null) put(LastBaseline, bandH + lastBase)
        }

        layout(
            width = width.coerceIn(constraints.minWidth, constraints.maxWidth),
            height = height.coerceIn(constraints.minHeight, constraints.maxHeight),
            alignmentLines = lines,
        ) {
            for (i in rubies.indices) {
                val r = rubies[i] ?: continue
                r.placeRelative((shift + starts[i]).roundToInt(), bandH - r.height)
            }
            var x = shiftPx
            for (b in bases) {
                b.placeRelative(x, bandH)
                x += b.width
            }
        }
    }
}
