package dev.officexr.bevyflutter

import android.view.Surface

/** Receives JSON events from the Bevy host, on a Rust thread. */
fun interface BevyEventListener {
    fun onEvent(json: String)
}

/**
 * The JNI surface of `libbevy_flutter_host.so` (`mobile-flutter/native/src/android.rs`).
 * One Bevy app runs at a time; `nativeStart` stops any previous one.
 */
object BevyBridge {
    init {
        System.loadLibrary("bevy_flutter_host")
    }

    external fun nativeStart(
        surface: Surface,
        width: Int,
        height: Int,
        scale: Float,
        /** A `SceneSpec` as JSON, e.g. `{"type":"demo"}`. */
        scene: String,
        listener: BevyEventListener,
    )

    /** `phase`: 0 started, 1 moved, 2 ended, 3 cancelled. Positions are logical pixels. */
    external fun nativeTouch(id: Long, phase: Int, x: Float, y: Float)

    external fun nativeResize(width: Int, height: Int)

    /** A `SceneCommand` as JSON, e.g. `{"type":"addCube"}`. */
    external fun nativeCommand(json: String)

    external fun nativeStop()
}
