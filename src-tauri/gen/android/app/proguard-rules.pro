# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.

# Bulbul JNI Bridge & Rust Interop models
-keep class com.bulbul.app.BulbulBridge { *; }
-keep class com.bulbul.app.ProcessAudioOptions { *; }
-keep class com.bulbul.app.ProcessAudioResult { *; }
-keep class com.bulbul.app.ContextModeItem { *; }
-keep class com.bulbul.app.VocabProfileItem { *; }
-keep class com.bulbul.app.HistoryItem { *; }

# Background Services, IME & Native Audio
-keep class com.bulbul.app.BulbulInputMethodService { *; }
-keep class com.bulbul.app.BulbulForegroundService { *; }
-keep class com.bulbul.app.AudioRecorder { *; }
-keep class com.bulbul.app.MainActivity { *; }