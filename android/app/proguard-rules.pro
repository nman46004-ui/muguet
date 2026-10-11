# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# Keep JNI classes
-keep class com.muguet.TouchOverlay { *; }
-keep class com.muguet.MainActivity { *; }
-keep class com.muguet.LauncherActivity { *; }
-keep class com.muguet.MuguetApplication { *; }

# Keep native methods
-keepclasseswithmembernames class * {
    native <methods>;
}

# Keep Rust-generated JNI bindings
-keep class org.mozilla.gecko.* { *; }