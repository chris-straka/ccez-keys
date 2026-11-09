package uk.ccez.keys

import android.content.ClipDescription
import android.content.ClipboardManager
import android.inputmethodservice.InputMethodService
import android.os.Build
import android.view.View
import android.view.inputmethod.EditorInfo
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.ComposeView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.setViewTreeLifecycleOwner
import androidx.savedstate.SavedStateRegistry
import androidx.savedstate.SavedStateRegistryController
import androidx.savedstate.SavedStateRegistryOwner
import androidx.savedstate.setViewTreeSavedStateRegistryOwner
import uk.ccez.keys.ui.KeyboardUi

enum class Panel { KEYS, FACE, SETTINGS, CLIPS, VOICE, DRAW }

/** What the Compose parts draw from. */
class UiState {
    var panel by mutableStateOf(Panel.KEYS)
    var bar by mutableStateOf<Bar>(Bar.Buttons)
    var popup by mutableStateOf<Popup?>(null)
    /** The newest copy, until it's pasted or something else is copied. */
    var clip by mutableStateOf<String?>(null)
    /** The buttons (clipboard, camera, mic, settings) shown over the words until the next key. */
    var tools by mutableStateOf(false)
    var chatApp by mutableStateOf(false)
    /** The strip's translations are polite in this app. */
    var polite by mutableStateOf(false)
    var password by mutableStateOf(false)
    var keysHeightDp by mutableFloatStateOf(212f)
    var strip by mutableStateOf<uk.ccez.keys.learn.StripView?>(null)
    var card by mutableStateOf<uk.ccez.keys.learn.CardView?>(null)
    var tip by mutableStateOf<uk.ccez.keys.learn.Tip?>(null)
    /** A kaomoji worth offering (chat apps). */
    var face by mutableStateOf<String?>(null)
    var voice by mutableStateOf<uk.ccez.keys.learn.VoiceState?>(null)
    /** An Ask chat is open: the keys type into it. */
    var ask by mutableStateOf<uk.ccez.keys.learn.AskState?>(null)
}

/** The keyboard. Compose needs a lifecycle, which an input method doesn't have, so it gets one. */
class KeysService : InputMethodService(), LifecycleOwner, SavedStateRegistryOwner {
    private val lifecycleRegistry = LifecycleRegistry(this)
    private val savedState = SavedStateRegistryController.create(this)
    override val lifecycle: Lifecycle get() = lifecycleRegistry
    override val savedStateRegistry: SavedStateRegistry get() = savedState.savedStateRegistry

    val ui = UiState()
    lateinit var typist: Typist
        private set
    private var clipboard: ClipboardManager? = null
    private val onClip = ClipboardManager.OnPrimaryClipChangedListener { readClip() }

    override fun onCreate() {
        super.onCreate()
        savedState.performRestore(null)
        lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_CREATE)
        typist = Typist(this, ui)
        App.instance.clearClipChip = { ui.clip = null }
        App.instance.keymap = { typist.view.keymap() }
        App.instance.onRecovered = { runCatching { ui.panel = Panel.KEYS; ui.popup = null; typist.recover() } }
        clipboard = getSystemService(ClipboardManager::class.java)?.also { it.addPrimaryClipChangedListener(onClip) }
        App.instance.log("info", "keyboard-created")
    }

    override fun onCreateInputView(): View {
        window.window?.decorView?.let {
            it.setViewTreeLifecycleOwner(this)
            it.setViewTreeSavedStateRegistryOwner(this)
        }
        // A tap on the app above closes a word card (the keyboard hears of it, the app gets the tap too).
        window.window?.let { w ->
            w.addFlags(android.view.WindowManager.LayoutParams.FLAG_WATCH_OUTSIDE_TOUCH)
            val inner = w.callback
            if (inner != null && inner !is OutsideTaps) w.callback = OutsideTaps(inner) { if (ui.card != null) ui.card = null }
        }
        return ComposeView(this).apply {
            setViewTreeLifecycleOwner(this@KeysService)
            setViewTreeSavedStateRegistryOwner(this@KeysService)
            setContent { KeyboardUi(ui, typist) }
        }
    }

    override fun onStartInputView(info: EditorInfo, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        if (lifecycleRegistry.currentState != Lifecycle.State.DESTROYED) lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_RESUME)
        typist.start(info)
        uk.ccez.keys.learn.WordSync.maybe()
        App.instance.pendingInsert?.let { t ->
            App.instance.pendingInsert = null
            typist.paste(t)
        }
    }

    override fun onFinishInputView(finishingInput: Boolean) {
        super.onFinishInputView(finishingInput)
        // The keyboard just closed: a good moment for an update.
        Updater.closed(this) { !isInputViewShown }
        typist.voice.close()
        typist.finish()
        if (lifecycleRegistry.currentState.isAtLeast(Lifecycle.State.RESUMED)) lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_PAUSE)
    }

    override fun onUpdateSelection(oldSelStart: Int, oldSelEnd: Int, newSelStart: Int, newSelEnd: Int, candidatesStart: Int, candidatesEnd: Int) {
        super.onUpdateSelection(oldSelStart, oldSelEnd, newSelStart, newSelEnd, candidatesStart, candidatesEnd)
        typist.onSelection(newSelStart, newSelEnd, candidatesStart, candidatesEnd)
    }

    override fun onDestroy() {
        clipboard?.removePrimaryClipChangedListener(onClip)
        // Android finishes the input view inside super.onDestroy (a pause): destroyed after that.
        super.onDestroy()
        lifecycleRegistry.handleLifecycleEvent(Lifecycle.Event.ON_DESTROY)
    }

    /** Text copied anywhere, unless the app marked it sensitive (password managers). */
    private fun readClip() {
        val clip = runCatching { clipboard?.primaryClip }.getOrNull() ?: return
        val sensitive = Build.VERSION.SDK_INT >= 33 &&
            clip.description.extras?.getBoolean(ClipDescription.EXTRA_IS_SENSITIVE) == true
        // A picture (a screenshot put there from the panel): no text chip for it.
        val picture = clip.description.hasMimeType("image/*") || (clip.itemCount > 0 && clip.getItemAt(0).uri != null && clip.getItemAt(0).text == null)
        if (sensitive || clip.itemCount == 0 || picture) { ui.clip = null; return }
        val text = clip.getItemAt(0).coerceToText(this)?.toString()?.trim().orEmpty().take(10_000)
        ui.clip = text.ifEmpty { null }
        if (text.isNotEmpty()) uk.ccez.keys.learn.Clips.add(this, text)
    }
}

/** Passes everything on; a touch outside the keyboard runs `onOutside` first. */
private class OutsideTaps(private val inner: android.view.Window.Callback, private val onOutside: () -> Unit) : android.view.Window.Callback by inner {
    override fun dispatchTouchEvent(event: android.view.MotionEvent): Boolean {
        if (event.actionMasked == android.view.MotionEvent.ACTION_OUTSIDE) { onOutside(); return true }
        return inner.dispatchTouchEvent(event)
    }
}
