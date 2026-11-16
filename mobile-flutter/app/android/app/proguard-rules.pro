# Only Rust calls into these over JNI (BevyEventListener.onEvent), so R8 can't see the uses.
-keep class dev.officexr.bevyflutter.** { *; }
