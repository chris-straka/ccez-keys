package uk.ccez.keys.learn

import android.Manifest
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.net.Uri
import android.provider.MediaStore
import android.util.Size
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import androidx.core.content.FileProvider
import androidx.core.view.inputmethod.EditorInfoCompat
import androidx.core.view.inputmethod.InputConnectionCompat
import androidx.core.view.inputmethod.InputContentInfoCompat
import java.io.File

/**
 * The latest screenshots, for the clipboard panel (Android doesn't put
 * them on the clipboard). A tap hands one to the app: as an image where
 * the field takes images (chat apps), else onto the clipboard to paste.
 */
object Shots {
    fun allowed(ctx: Context) = ctx.checkSelfPermission(Manifest.permission.READ_MEDIA_IMAGES) == PackageManager.PERMISSION_GRANTED

    /** Screenshots of the last day, newest first. */
    fun recent(ctx: Context, n: Int = 6): List<Uri> {
        if (!allowed(ctx)) return emptyList()
        val since = System.currentTimeMillis() / 1000 - 24 * 3600
        return runCatching {
            ctx.contentResolver.query(
                MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                arrayOf(MediaStore.Images.Media._ID),
                "${MediaStore.Images.Media.RELATIVE_PATH} LIKE ? AND ${MediaStore.Images.Media.DATE_ADDED} > ?",
                arrayOf("%Screenshots%", since.toString()),
                "${MediaStore.Images.Media.DATE_ADDED} DESC",
            )?.use { c ->
                val out = mutableListOf<Uri>()
                while (c.moveToNext() && out.size < n) out += ContentUris.withAppendedId(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, c.getLong(0))
                out
            }.orEmpty()
        }.getOrDefault(emptyList())
    }

    /** The newest screenshot of the last day, and when it was taken (ms). */
    fun newest(ctx: Context): Pair<Uri, Long>? {
        if (!allowed(ctx)) return null
        val since = System.currentTimeMillis() / 1000 - 24 * 3600
        return runCatching {
            ctx.contentResolver.query(
                MediaStore.Images.Media.EXTERNAL_CONTENT_URI,
                arrayOf(MediaStore.Images.Media._ID, MediaStore.Images.Media.DATE_ADDED),
                "${MediaStore.Images.Media.RELATIVE_PATH} LIKE ? AND ${MediaStore.Images.Media.DATE_ADDED} > ?",
                arrayOf("%Screenshots%", since.toString()),
                "${MediaStore.Images.Media.DATE_ADDED} DESC",
            )?.use { c -> if (c.moveToFirst()) ContentUris.withAppendedId(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, c.getLong(0)) to c.getLong(1) * 1000 else null }
        }.getOrNull()
    }

    /** The picture big enough to fill the keyboard. */
    fun large(ctx: Context, uri: Uri): Bitmap? = runCatching { ctx.contentResolver.loadThumbnail(uri, Size(1440, 2560), null) }.getOrNull()

    fun thumb(ctx: Context, uri: Uri): Bitmap? = runCatching { ctx.contentResolver.loadThumbnail(uri, Size(240, 480), null) }.getOrNull()

    /** True when the app took it as an image; false: it's on the clipboard instead. */
    fun send(ctx: Context, ic: InputConnection, editor: EditorInfo, uri: Uri): Boolean {
        val dir = File(ctx.cacheDir, "shots").apply { mkdirs() }
        dir.listFiles()?.forEach { it.delete() }
        val file = File(dir, "screenshot-${System.currentTimeMillis()}.png")
        ctx.contentResolver.openInputStream(uri)?.use { i -> file.outputStream().use { i.copyTo(it) } } ?: return false
        val mine = FileProvider.getUriForFile(ctx, "uk.ccez.keys.files", file)
        val takes = EditorInfoCompat.getContentMimeTypes(editor).any { ClipDescription.compareMimeTypes("image/png", it) }
        if (takes) {
            val info = InputContentInfoCompat(mine, ClipDescription("screenshot", arrayOf("image/png")), null)
            if (InputConnectionCompat.commitContent(ic, editor, info, InputConnectionCompat.INPUT_CONTENT_GRANT_READ_URI_PERMISSION, null)) return true
        }
        ctx.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newUri(ctx.contentResolver, "screenshot", mine))
        return false
    }
}
