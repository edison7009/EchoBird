package com.echobird.ai

import android.os.Bundle
import android.view.WindowInsetsController
import android.webkit.WebChromeClient
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    // Force light status bar icons (white text on dark background)
    // Required for MIUI/ColorOS/OneUI which may ignore themes.xml
    window.decorView.post {
      if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
        // Android 11+ (API 30): use WindowInsetsController
        window.insetsController?.setSystemBarsAppearance(
          0, // clear LIGHT flags → white icons
          WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or
            WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
        )
      } else {
        // Android 10 and below: use legacy View flags
        @Suppress("DEPRECATION")
        window.decorView.systemUiVisibility =
          window.decorView.systemUiVisibility and
            android.view.View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR.inv() and
            android.view.View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR.inv()
      }

      // Listen for keyboard (IME) insets and inject height into WebView CSS
      val rootView = window.decorView.rootView
      ViewCompat.setOnApplyWindowInsetsListener(rootView) { view, insets ->
        val imeInsets = insets.getInsets(WindowInsetsCompat.Type.ime())
        val imeHeight = imeInsets.bottom
        val imeVisible = insets.isVisible(WindowInsetsCompat.Type.ime())
        val webView = findWebView(rootView)
        if (webView != null) {
          val density = resources.displayMetrics.density
          val appHeight = (view.height - imeHeight) / density
          webView.evaluateJavascript(
            "document.documentElement.style.setProperty('--keyboard-height', '${imeHeight}px');" +
            "document.documentElement.style.setProperty('--app-height', '${appHeight}px');" +
            "var el = document.querySelector('.mobile-app');" +
            "if(el){el.classList.${if (imeVisible) "add" else "remove"}('keyboard-visible');}",
            null
          )
        }
        ViewCompat.onApplyWindowInsets(view, insets)
      }
    }
  }

  // Recursively find the WebView in the view hierarchy
  private fun findWebView(view: android.view.View): WebView? {
    if (view is WebView) return view
    if (view is android.view.ViewGroup) {
      for (i in 0 until view.childCount) {
        val found = findWebView(view.getChildAt(i))
        if (found != null) return found
      }
    }
    return null
  }
}
