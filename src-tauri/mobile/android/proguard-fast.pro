# The page calls these by name through window.FastNative, so they must keep their names.
-keep class __PACKAGE__.FastNative { *; }
-keepclassmembers class __PACKAGE__.FastNative {
  @android.webkit.JavascriptInterface <methods>;
}
