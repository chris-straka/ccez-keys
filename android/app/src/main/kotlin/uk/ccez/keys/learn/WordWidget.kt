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
import uk.ccez.keys.App
import uk.ccez.keys.R

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
                Review.known(ctx, lang, word)
                refresh(ctx, advance = true)
            }
            LATER -> now?.let { (lang, word) ->
                Review.later(ctx, lang, word)
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
        private fun prefs(ctx: Context) = ctx.getSharedPreferences("widget", Context.MODE_PRIVATE)
        private fun current(ctx: Context): Pair<String, String>? =
            prefs(ctx).getString("current", null)?.split('|', limit = 2)?.takeIf { it.size == 2 }?.let { it[0] to it[1] }

        /** Shows a word (the next one when `advance`), its meaning looked up off the main thread. */
        fun refresh(ctx: Context, advance: Boolean) {
            val app = App.instance
            app.scope.launch {
                val shown = current(ctx)
                val pick = if (!advance && shown != null && shown in Review.due(ctx)) shown else Review.next(ctx, shown)
                prefs(ctx).edit().putString("current", pick?.let { "${it.first}|${it.second}" }).apply()
                val view = RemoteViews(ctx.packageName, R.layout.learn_widget)
                if (pick == null) {
                    view.setTextViewText(R.id.flag, "")
                    view.setTextViewText(R.id.reading, "")
                    view.setTextViewText(R.id.word, "—")
                    view.setTextViewText(R.id.meaning, "No words in Learning")
                    listOf(R.id.known, R.id.later).forEach { view.setViewVisibility(it, View.INVISIBLE) }
                } else {
                    val f = Review.face(ctx, pick.first, pick.second)
                    view.setTextViewText(R.id.flag, f.flag)
                    view.setTextViewText(R.id.reading, listOf(f.reading, f.form).filter { it.isNotEmpty() }.joinToString("  "))
                    view.setTextViewText(R.id.word, (if (f.article.isEmpty()) "" else if (f.article.endsWith("'")) f.article else "${f.article} ") + f.word)
                    view.setTextViewText(R.id.meaning, f.meaning)
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
