# JNA and the UniFFI bindings are reached by reflection from native code.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uk.ccez.keys.core.** { *; }
-dontwarn java.awt.**
# ML Kit builds its components by reflection from no-arg constructors.
-keep class com.google.mlkit.**.*Registrar { <init>(); }
-keep class * implements com.google.firebase.components.ComponentRegistrar { <init>(); }
