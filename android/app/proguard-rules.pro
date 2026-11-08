# JNA and the UniFFI bindings are reached by reflection from native code.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uk.ccez.keys.core.** { *; }
-dontwarn java.awt.**
# ML Kit builds its components by reflection from no-arg constructors.
-keep class com.google.mlkit.**.*Registrar { <init>(); }
-keep class * implements com.google.firebase.components.ComponentRegistrar { <init>(); }
# WorkManager (ML Kit's model downloads) builds its Room database by reflection.
-keep class * extends androidx.room.RoomDatabase { <init>(); }
-keep class androidx.work.impl.** { *; }
