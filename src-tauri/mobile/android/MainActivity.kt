package __PACKAGE__

// The app's one Android screen. Tauri's own activity does nearly everything;
// this adds what a file manager needs from the system and a web page cannot
// reach by itself:
//
//   - asking for "All files access", without which Android shows an app
//     nothing of shared storage
//   - handing a file to another app to open, and the share sheet
//   - keeping the page clear of the status bar, the navigation bar and the
//     keyboard, and coloring the strips behind the bars like the page
//
// The page reaches these through `window.FastNative` (see src/util/native.ts).
// scripts/mobile-setup.mjs copies this file over the generated MainActivity.

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.MimeTypeMap
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import java.io.File

class MainActivity : TauriActivity() {
  private var page: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    page = webView
    webView.addJavascriptInterface(FastNative(this), "FastNative")
    // Since Android 15 every app draws behind the system bars. Keep the page out from
    // under them, and above the keyboard, by giving the web view matching margins.
    ViewCompat.setOnApplyWindowInsetsListener(webView) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime()
      )
      (view.layoutParams as? ViewGroup.MarginLayoutParams)?.let { p ->
        if (p.leftMargin != bars.left || p.topMargin != bars.top || p.rightMargin != bars.right || p.bottomMargin != bars.bottom) {
          p.setMargins(bars.left, bars.top, bars.right, bars.bottom)
          view.layoutParams = p
        }
      }
      WindowInsetsCompat.CONSUMED
    }
  }

  override fun onResume() {
    super.onResume()
    // Coming back from the permission page: let the page look again.
    page?.post { page?.evaluateJavascript("window.dispatchEvent(new Event('fast-storage-access'))", null) }
  }

  fun hasAllFilesAccess(): Boolean =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) Environment.isExternalStorageManager()
    else ContextCompat.checkSelfPermission(this, Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED

  fun requestAllFilesAccess() {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      try {
        startActivity(Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:$packageName")))
      } catch (e: Exception) {
        // Some phones only have the page that lists every app.
        try {
          startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
        } catch (e2: Exception) {
          startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
        }
      }
    } else {
      ActivityCompat.requestPermissions(
        this,
        arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE),
        7001
      )
    }
  }

  override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<String>, grantResults: IntArray) {
    super.onRequestPermissionsResult(requestCode, permissions, grantResults)
    if (requestCode == 7001) {
      page?.post { page?.evaluateJavascript("window.dispatchEvent(new Event('fast-storage-access'))", null) }
    }
  }

  private fun shareable(path: String): Uri = FileProvider.getUriForFile(this, "$packageName.fileprovider", File(path))

  private fun typeOf(path: String): String {
    val ext = path.substringAfterLast('.', "").lowercase()
    return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "*/*"
  }

  /** "" when another app took the file, else the reason in words. */
  fun openFile(path: String): String {
    return try {
      val uri = shareable(path)
      val view = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, typeOf(path))
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
      startActivity(Intent.createChooser(view, null))
      ""
    } catch (e: ActivityNotFoundException) {
      "No app on this device can open this kind of file."
    } catch (e: Exception) {
      "Could not open the file: ${e.message ?: e.javaClass.simpleName}"
    }
  }

  fun shareFiles(paths: List<String>): String {
    if (paths.isEmpty()) return "Nothing to share."
    return try {
      val uris = ArrayList(paths.map { shareable(it) })
      val types = paths.map { typeOf(it) }.distinct()
      val send = if (uris.size == 1) {
        Intent(Intent.ACTION_SEND).putExtra(Intent.EXTRA_STREAM, uris[0])
      } else {
        Intent(Intent.ACTION_SEND_MULTIPLE).putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
      }
      send.type = if (types.size == 1) types[0] else "*/*"
      // The share sheet shows previews only for files named in the clip data.
      val clip = ClipData.newUri(contentResolver, "files", uris[0])
      for (i in 1 until uris.size) clip.addItem(ClipData.Item(uris[i]))
      send.clipData = clip
      send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      startActivity(Intent.createChooser(send, null))
      ""
    } catch (e: Exception) {
      "Could not share: ${e.message ?: e.javaClass.simpleName}"
    }
  }

  fun setBars(color: String, lightBackground: Boolean) {
    runOnUiThread {
      try {
        window.decorView.setBackgroundColor(Color.parseColor(color))
        val bars = WindowCompat.getInsetsController(window, window.decorView)
        bars.isAppearanceLightStatusBars = lightBackground
        bars.isAppearanceLightNavigationBars = lightBackground
      } catch (e: Exception) {
        // A color the system cannot read: leave the bars as they are.
      }
    }
  }
}

/** What the page may call. Calls arrive on a background thread, so anything that shows UI hops to the main one. */
class FastNative(private val activity: MainActivity) {
  @JavascriptInterface
  fun maker(): String = Build.MANUFACTURER ?: ""

  @JavascriptInterface
  fun model(): String = Build.MODEL ?: ""

  @JavascriptInterface
  fun hasAllFilesAccess(): Boolean = activity.hasAllFilesAccess()

  @JavascriptInterface
  fun requestAllFilesAccess() = activity.runOnUiThread { activity.requestAllFilesAccess() }

  @JavascriptInterface
  fun openFile(path: String): String = activity.openFile(path)

  @JavascriptInterface
  fun shareFiles(paths: String): String = activity.shareFiles(paths.split("\n").filter { it.isNotBlank() })

  @JavascriptInterface
  fun setBars(color: String, lightBackground: Boolean) = activity.setBars(color, lightBackground)
}
