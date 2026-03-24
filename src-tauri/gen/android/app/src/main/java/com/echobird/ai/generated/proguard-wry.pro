# THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!!

# Copyright 2020-2023 Tauri Programme within The Commons Conservancy
# SPDX-License-Identifier: Apache-2.0
# SPDX-License-Identifier: MIT

-keep class com.echobird.ai.* {
  native <methods>;
}

-keep class com.echobird.ai.WryActivity {
  public <init>(...);

  void setWebView(com.echobird.ai.RustWebView);
  java.lang.Class getAppClass(...);
  java.lang.String getVersion();
}

-keep class com.echobird.ai.Ipc {
  public <init>(...);

  @android.webkit.JavascriptInterface public <methods>;
}

-keep class com.echobird.ai.RustWebView {
  public <init>(...);

  void loadUrlMainThread(...);
  void loadHTMLMainThread(...);
  void evalScript(...);
}

-keep class com.echobird.ai.RustWebChromeClient,com.echobird.ai.RustWebViewClient {
  public <init>(...);
}
