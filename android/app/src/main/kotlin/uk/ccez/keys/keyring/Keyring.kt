package uk.ccez.keys.keyring

import android.app.Activity
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.database.Cursor
import android.net.Uri
import android.os.Binder
import android.os.Build
import android.os.Bundle
import android.os.Process
import android.os.UserManager
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import java.security.MessageDigest
import uk.ccez.keys.App
import uk.ccez.keys.Colors
import uk.ccez.keys.learn.Secrets

/**
 * The AI key for the owner's other apps (docs/keyring.md). One method,
 * `key`: an app asks for a provider's key ("muse") and gets it back if it
 * was allowed, else "approve" (it opens [Approve] for the owner) or a
 * reason. Who asks is the system's word (the calling uid), never anything
 * the caller sends. Allowing remembers the app's signing certificate, so a
 * different app under the same name gets nothing.
 *
 * Kept apart from the keyboard: only [Secrets] and [App.log] are shared.
 */
class Provider : ContentProvider() {
    override fun onCreate() = true

    override fun call(method: String, arg: String?, extras: Bundle?): Bundle {
        val ctx = context ?: throw SecurityException("no context")
        val uid = Binder.getCallingUid()
        val pkg = Keyring.caller(ctx, uid, callingPackage) ?: throw SecurityException("unknown caller")
        if (method != "key") throw SecurityException("no method $method")
        val out = Bundle()
        val status = Keyring.answer(ctx, pkg, arg ?: "muse", out)
        out.putString("status", status)
        log("keyring", "app" to pkg, "status" to status)
        return out
    }

    // Nothing else is served.
    override fun query(uri: Uri, p: Array<out String>?, s: String?, a: Array<out String>?, o: String?): Cursor? = null
    override fun getType(uri: Uri): String? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, s: String?, a: Array<out String>?) = 0
    override fun update(uri: Uri, v: ContentValues?, s: String?, a: Array<out String>?) = 0
}

object Keyring {
    private fun prefs(ctx: Context) = ctx.getSharedPreferences("keyring", Context.MODE_PRIVATE)

    /**
     * The one package behind [uid] (a shared uid is refused: the answer
     * would go to every app in it), which must be the package the
     * system reported calling.
     */
    fun caller(ctx: Context, uid: Int, reported: String?): String? {
        if (uid == Process.myUid()) return null
        val pkgs = ctx.packageManager.getPackagesForUid(uid) ?: return null
        val pkg = pkgs.singleOrNull() ?: return null
        return pkg.takeIf { reported == null || it == reported }
    }

    /** The SHA-256 of [pkg]'s current signing certificate, as hex. */
    fun digest(ctx: Context, pkg: String): String? = runCatching {
        if (Build.VERSION.SDK_INT < 28) return null
        val info = ctx.packageManager.getPackageInfo(pkg, PackageManager.GET_SIGNING_CERTIFICATES).signingInfo ?: return null
        // One signer, possibly rotated: the newest certificate. Several signers: refused.
        val cert = if (info.hasMultipleSigners()) return null else info.signingCertificateHistory.lastOrNull() ?: return null
        MessageDigest.getInstance("SHA-256").digest(cert.toByteArray()).joinToString("") { "%02x".format(it) }
    }.getOrNull()

    /** Whether [pkg] is signed by the certificate [hex] (rotations included). */
    private fun signedBy(ctx: Context, pkg: String, hex: String): Boolean = runCatching {
        if (Build.VERSION.SDK_INT < 28) return false
        val bytes = hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        ctx.packageManager.hasSigningCertificate(pkg, bytes, PackageManager.CERT_INPUT_SHA256)
    }.getOrDefault(false)

    fun allowed(ctx: Context, pkg: String) = prefs(ctx).getString("allow:$pkg", null)?.let { signedBy(ctx, pkg, it) } == true
    private fun denied(ctx: Context, pkg: String) = prefs(ctx).getString("deny:$pkg", null)?.let { signedBy(ctx, pkg, it) } == true

    /** Fills [out] for [pkg]'s ask for [provider]'s key; returns the status. */
    fun answer(ctx: Context, pkg: String, provider: String, out: Bundle): String {
        if (ctx.getSystemService(UserManager::class.java)?.isUserUnlocked == false) return "locked"
        if (denied(ctx, pkg)) return "denied"
        if (!allowed(ctx, pkg)) return "approve"
        val held = runCatching { App.instance.engine.setting("ai.provider").trim('"') }.getOrDefault("")
        val key = Secrets.get(ctx)
        if (held != provider || key == null) return "none"
        out.putString("key", key)
        out.putInt("version", Secrets.version(ctx))
        return "ok"
    }

    /** Allows (or, with [allow] false, refuses) [pkg] as it's signed now. */
    fun decide(ctx: Context, pkg: String, allow: Boolean): Boolean {
        val d = digest(ctx, pkg) ?: return false
        prefs(ctx).edit().remove("allow:$pkg").remove("deny:$pkg").putString(if (allow) "allow:$pkg" else "deny:$pkg", d).apply()
        log("keyring-decided", "app" to pkg, "allow" to allow)
        return true
    }

    /** Forgets [pkg]: it asks again next time. */
    fun forget(ctx: Context, pkg: String) {
        prefs(ctx).edit().remove("allow:$pkg").remove("deny:$pkg").apply()
        log("keyring-forgot", "app" to pkg)
    }

    /** Apps allowed, then refused: package to allowed. */
    fun decided(ctx: Context): List<Pair<String, Boolean>> = prefs(ctx).all.keys.mapNotNull { k ->
        when {
            k.startsWith("allow:") -> k.removePrefix("allow:") to true
            k.startsWith("deny:") -> k.removePrefix("deny:") to false
            else -> null
        }
    }.sortedBy { !it.second }

    fun label(ctx: Context, pkg: String): String = runCatching {
        ctx.packageManager.getApplicationLabel(ctx.packageManager.getApplicationInfo(pkg, 0)).toString()
    }.getOrDefault(pkg)
}

private fun log(event: String, vararg fields: Pair<String, Any?>) {
    runCatching { App.instance.log("info", event, *fields) }
}

/**
 * The owner's yes or no for one app, opened by that app for a result
 * (a keyboard in the background may not open screens). The app is the
 * one the system says started this; it's checked again here.
 */
class Approve : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Nothing drawn over this may trick a tap.
        if (Build.VERSION.SDK_INT >= 31) window.setHideOverlayWindows(true)
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        val pkg = callingPackage
        val digest = pkg?.let { Keyring.digest(this, it) }
        if (pkg == null || pkg == packageName || digest == null) {
            setResult(RESULT_CANCELED)
            finish()
            return
        }
        if (Keyring.allowed(this, pkg)) {
            setResult(RESULT_OK)
            finish()
            return
        }
        val name = Keyring.label(this, pkg)
        val dp = resources.displayMetrics.density
        fun text(s: String, size: Float, color: Int) = TextView(this).apply {
            text = s
            textSize = size
            setTextColor(color)
            setPadding(0, (6 * dp).toInt(), 0, (6 * dp).toInt())
        }
        fun button(s: String, on: Boolean, tap: () -> Unit) = Button(this).apply {
            text = s
            isAllCaps = false
            setTextColor(if (on) Colors.ACCENT else Colors.FG)
            setBackgroundColor(Colors.KEY)
            filterTouchesWhenObscured = true
            setOnClickListener { tap() }
            layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f).apply { marginStart = (6 * dp).toInt(); marginEnd = (6 * dp).toInt() }
        }
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Colors.KB_BG)
            val pad = (24 * dp).toInt()
            setPadding(pad, pad, pad, pad)
            gravity = Gravity.CENTER_HORIZONTAL
            filterTouchesWhenObscured = true
            addView(text(name, 22f, Colors.FG))
            addView(text("wants your AI key", 15f, Colors.MUTED))
            addView(text(pkg + "\n" + digest.take(16).chunked(4).joinToString(" "), 12f, Colors.DIM).apply { typeface = android.graphics.Typeface.MONOSPACE })
            addView(LinearLayout(this@Approve).apply {
                orientation = LinearLayout.HORIZONTAL
                setPadding(0, (18 * dp).toInt(), 0, 0)
                addView(button("Don't allow", false) { decide(pkg, false) })
                addView(button("Allow", true) { decide(pkg, true) })
            })
        }
        root.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
        setContentView(root)
    }

    private fun decide(pkg: String, allow: Boolean) {
        val ok = Keyring.decide(this, pkg, allow)
        setResult(if (ok && allow) RESULT_OK else RESULT_CANCELED)
        finish()
    }
}
