package uk.ccez.keys.learn

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.view.View
import android.widget.RemoteViews
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import uk.ccez.keys.App
import uk.ccez.keys.R
import uk.ccez.keys.core.WordStatus
import uk.ccez.keys.ui.Langs

/**
 * The home-screen widget: one word saved as Learning (from every app, through
 * ccez-words), with its reading and meaning. A new one every 5 minutes
 * while the phone is awake (its own timer: Android's widget one is 30
 * minutes at best and late), or on Next. Known marks it known everywhere; Later hides it for a while that
 * grows each time (1, 3, 7, 14, then 30 days), like Anki's "easy". Tap the
 * word to hear it.
 */
class WordWidget : AppWidgetProvider() {
    override fun onUpdate(ctx: Context, manager: AppWidgetManager, ids: IntArray) {
        schedule(ctx)
        refresh(ctx, advance = true)
    }

    override fun onEnabled(ctx: Context) = schedule(ctx)

    override fun onDisabled(ctx: Context) {
        ctx.getSystemService(android.app.AlarmManager::class.java)?.cancel(tick(ctx))
    }

    override fun onReceive(ctx: Context, intent: Intent) {
        super.onReceive(ctx, intent)
        val now = current(ctx)
        when (intent.action) {
            NEXT -> refresh(ctx, advance = true)
            KNOWN -> now?.let { (lang, word) ->
                App.instance.engine.setWordStatus(lang, word, WordStatus.KNOWN)
                WordSync.soon()
                later(ctx).edit().remove("$lang|$word").apply()
                refresh(ctx, advance = true)
            }
            LATER -> now?.let { (lang, word) ->
                val p = later(ctx)
                val step = (p.getString("$lang|$word", null)?.substringAfter(',')?.toIntOrNull() ?: -1) + 1
                val days = DAYS[step.coerceAtMost(DAYS.lastIndex)]
                p.edit().putString("$lang|$word", "${System.currentTimeMillis() + days * DAY},$step").apply()
                refresh(ctx, advance = true)
            }
            SAY -> now?.let { (lang, word) -> Speaker.say(App.instance, word, lang) }
        }
    }

    companion object {
        private const val NEXT = "uk.ccez.keys.widget.NEXT"
        private const val KNOWN = "uk.ccez.keys.widget.KNOWN"
        private const val LATER = "uk.ccez.keys.widget.LATER"
        private const val SAY = "uk.ccez.keys.widget.SAY"
        private val DAYS = listOf(1L, 3L, 7L, 14L, 30L)
        /** How often a new word comes in. */
        private const val EVERY = 5 * 60 * 1000L

        private fun tick(ctx: Context) = PendingIntent.getBroadcast(
            ctx, 9, Intent(ctx, WordWidget::class.java).setAction(NEXT), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        /** The timer: never wakes the phone (asleep, the word waits for the screen). */
        fun schedule(ctx: Context) {
            val am = ctx.getSystemService(android.app.AlarmManager::class.java) ?: return
            am.setInexactRepeating(android.app.AlarmManager.ELAPSED_REALTIME, android.os.SystemClock.elapsedRealtime() + EVERY, EVERY, tick(ctx))
        }
        private const val DAY = 24 * 3600 * 1000L

        private fun prefs(ctx: Context) = ctx.getSharedPreferences("widget", Context.MODE_PRIVATE)
        /** "lang|word" → "due millis,step" for words put off with Later. */
        private fun later(ctx: Context) = ctx.getSharedPreferences("widget-later", Context.MODE_PRIVATE)

        private fun current(ctx: Context): Pair<String, String>? =
            prefs(ctx).getString("current", null)?.split('|', limit = 2)?.takeIf { it.size == 2 }?.let { it[0] to it[1] }

        /** The words due now (not put off), the shown one last so Next moves on. */
        private fun due(ctx: Context): List<Pair<String, String>> {
            val now = System.currentTimeMillis()
            val off = later(ctx)
            return App.instance.engine.learningWords()
                .map { it.lang to it.word }
                .filter { (l, w) -> (off.getString("$l|$w", null)?.substringBefore(',')?.toLongOrNull() ?: 0L) <= now }
        }

        /** Shows a word (the next one when `advance`), its meaning looked up off the main thread. */
        fun refresh(ctx: Context, advance: Boolean) {
            val app = App.instance
            app.scope.launch {
                val words = due(ctx)
                val shown = current(ctx)
                val pick = when {
                    words.isEmpty() -> null
                    !advance && shown != null && shown in words -> shown
                    else -> {
                        // Round the list in a shuffled order kept in prefs, so every word comes up.
                        val p = prefs(ctx)
                        var order = p.getString("order", "").orEmpty().split('\n').filter { it.isNotEmpty() }
                        val keys = words.map { "${it.first}|${it.second}" }.toSet()
                        order = order.filter { it in keys }
                        if (order.isEmpty()) order = keys.shuffled().let { l -> if (l.size > 1 && l.first() == shown?.let { "${it.first}|${it.second}" }) l.drop(1) + l.first() else l }
                        val k = order.first()
                        p.edit().putString("order", order.drop(1).joinToString("\n")).apply()
                        k.split('|', limit = 2).let { it[0] to it[1] }
                    }
                }
                prefs(ctx).edit().putString("current", pick?.let { "${it.first}|${it.second}" }).apply()
                val view = RemoteViews(ctx.packageName, R.layout.learn_widget)
                if (pick == null) {
                    view.setTextViewText(R.id.flag, "")
                    view.setTextViewText(R.id.reading, "")
                    view.setTextViewText(R.id.word, "—")
                    view.setTextViewText(R.id.meaning, "No words in Learning")
                    listOf(R.id.known, R.id.later).forEach { view.setViewVisibility(it, View.INVISIBLE) }
                } else {
                    val (lang, word) = pick
                    // Reading and meaning from the dictionary; the phone's translator when it has none.
                    val card = runCatching { app.engine.wordCard(lang, word, "") }.getOrNull()
                    val meaning = card?.meanings?.firstOrNull()
                        ?: withTimeoutOrNull(4000) { Translator.translate(word, lang, app.text("languages.default"), download = false) }
                        ?: ""
                    view.setTextViewText(R.id.flag, Langs.of(ctx, lang)?.flag ?: lang)
                    view.setTextViewText(R.id.reading, card?.reading?.takeIf { it.isNotEmpty() && it != word }.orEmpty())
                    view.setTextViewText(R.id.word, (card?.article?.takeIf { it.isNotEmpty() }?.let { if (it.endsWith("'")) it else "$it " } ?: "") + word)
                    view.setTextViewText(R.id.meaning, meaning)
                    listOf(R.id.known, R.id.later).forEach { view.setViewVisibility(it, View.VISIBLE) }
                }
                fun tap(action: String, code: Int) = PendingIntent.getBroadcast(
                    ctx, code, Intent(ctx, WordWidget::class.java).setAction(action), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                )
                view.setOnClickPendingIntent(R.id.next, tap(NEXT, 1))
                view.setOnClickPendingIntent(R.id.known, tap(KNOWN, 2))
                view.setOnClickPendingIntent(R.id.later, tap(LATER, 3))
                // Anywhere on the word: its card (said as it opens).
                val card = pick?.let { (l, w) ->
                    PendingIntent.getActivity(ctx, 5, WordCardActivity.intent(ctx, l, w), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
                }
                card?.let { view.setOnClickPendingIntent(R.id.root, it) }
                val manager = AppWidgetManager.getInstance(ctx)
                manager.updateAppWidget(ComponentName(ctx, WordWidget::class.java), view)
            }
        }
    }
}
