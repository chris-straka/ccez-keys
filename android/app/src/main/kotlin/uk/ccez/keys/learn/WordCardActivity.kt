package uk.ccez.keys.learn

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uk.ccez.keys.App
import uk.ccez.keys.core.Card
import uk.ccez.keys.core.WordStatus
import uk.ccez.keys.Colors
import uk.ccez.keys.ui.Icons
import uk.ccez.keys.ui.Langs
import uk.ccez.keys.ui.StatusButton

/**
 * The word card over the home screen, from the widget: the word (said as it
 * opens), its reading and meanings, Learning and Known. A tap outside closes it.
 */
class WordCardActivity : ComponentActivity() {
    @OptIn(ExperimentalFoundationApi::class)
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val lang = intent.getStringExtra("lang") ?: return finish()
        val word = intent.getStringExtra("word") ?: return finish()
        val app = App.instance
        Speaker.say(app, word, lang)
        setContent {
            var card by remember { mutableStateOf<Card?>(null) }
            var meaning by remember { mutableStateOf<String?>(null) }
            var status by remember { mutableStateOf(app.engine.wordStatus(lang, word)) }
            LaunchedEffect(Unit) {
                withContext(Dispatchers.IO) {
                    card = runCatching { app.engine.wordCard(lang, word, "") }.getOrNull()
                    if (card?.meanings.isNullOrEmpty()) meaning = Translator.translate(word, lang, app.text("languages.default"), download = false)
                }
            }
            Box(
                Modifier.fillMaxSize().background(Color(0x99000000)).combinedClickable(onClick = { finish() }),
                contentAlignment = Alignment.Center,
            ) {
                Column(
                    Modifier.padding(20.dp).fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Color(0xFF1D1D1F))
                        .combinedClickable(onClick = {}).padding(horizontal = 16.dp, vertical = 14.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    val c = card
                    val head = c?.lemma?.ifEmpty { null } ?: word
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(Langs.of(this@WordCardActivity, lang)?.flag ?: lang, fontSize = 13.sp, modifier = Modifier.padding(end = 6.dp).graphicsLayer { alpha = 0.55f })
                        c?.reading?.takeIf { it.isNotEmpty() && it != head }?.let { Text(it, color = Colors.muted, fontSize = 13.sp) }
                        Spacer(Modifier.weight(1f))
                        c?.pos.orEmpty().take(2).forEach { p ->
                            Text(p, color = Colors.muted, fontSize = 10.sp, modifier = Modifier.padding(start = 4.dp).clip(RoundedCornerShape(4.dp)).background(Color(0xFF2A2A2C)).padding(horizontal = 5.dp, vertical = 1.dp))
                        }
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        val article = c?.article.orEmpty()
                        if (article.isNotEmpty()) Text(if (article.endsWith("'")) article else "$article ", color = Colors.dim, fontSize = 22.sp)
                        Text(head, color = Colors.fg, fontSize = 28.sp, fontWeight = FontWeight.SemiBold)
                        Box(
                            Modifier.padding(start = 8.dp).size(34.dp).clip(RoundedCornerShape(17.dp)).combinedClickable(onClick = { Speaker.say(app, head, lang) }),
                            contentAlignment = Alignment.Center,
                        ) { Icon(Icons.sound, "Listen", tint = Colors.muted, modifier = Modifier.size(18.dp)) }
                    }
                    val meanings = c?.meanings.orEmpty().ifEmpty { listOfNotNull(meaning) }
                    meanings.take(3).forEach { Text(it, color = Colors.fg, fontSize = 15.sp, maxLines = 2) }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        StatusButton("Learning", status == WordStatus.LEARNING) {
                            status = if (status == WordStatus.LEARNING) WordStatus.NEW else WordStatus.LEARNING
                            app.engine.setWordStatus(lang, head, status); WordSync.soon()
                        }
                        StatusButton("Known", status == WordStatus.KNOWN) {
                            status = if (status == WordStatus.KNOWN) WordStatus.NEW else WordStatus.KNOWN
                            app.engine.setWordStatus(lang, head, status); WordSync.soon()
                            WordWidget.refresh(app, advance = true)
                            finish()
                        }
                    }
                }
            }
        }
    }

    companion object {
        fun intent(ctx: Context, lang: String, word: String) =
            Intent(ctx, WordCardActivity::class.java).putExtra("lang", lang).putExtra("word", word)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK or Intent.FLAG_ACTIVITY_NO_ANIMATION)
    }
}
