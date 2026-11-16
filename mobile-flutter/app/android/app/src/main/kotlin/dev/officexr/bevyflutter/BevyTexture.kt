package dev.officexr.bevyflutter

import android.os.Handler
import android.os.Looper
import android.util.Log
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import io.flutter.view.TextureRegistry

/**
 * Bridges Dart to the Bevy host: a Flutter texture backed by a `SurfaceProducer`, which Bevy
 * renders into, plus channels for input, commands and events.
 *
 * Methods on `officexr/bevy`: `start {width, height, scale, scene}` (physical size and a
 * `SceneSpec` JSON; returns the texture id), `resize {width, height}`, `touch {id, phase, x, y}`,
 * `command {json}`, `stop`, and `launchArguments` (the intent's `officexr_args`).
 * Events on `officexr/bevy/events` are the host's JSON strings.
 */
class BevyTexture(
    messenger: BinaryMessenger,
    private val textures: TextureRegistry,
    private val launchArguments: List<String>,
) : MethodChannel.MethodCallHandler, EventChannel.StreamHandler {
    private val main = Handler(Looper.getMainLooper())
    private var producer: TextureRegistry.SurfaceProducer? = null
    private var scale = 1f
    private var scene = "{\"type\":\"demo\"}"
    private var sink: EventChannel.EventSink? = null

    init {
        MethodChannel(messenger, "officexr/bevy").setMethodCallHandler(this)
        EventChannel(messenger, "officexr/bevy/events").setStreamHandler(this)
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "start" -> {
                val width = call.argument<Int>("width")!!
                val height = call.argument<Int>("height")!!
                scale = (call.argument<Double>("scale") ?: 1.0).toFloat()
                scene = call.argument<String>("scene") ?: scene
                result.success(start(width, height))
            }
            "resize" -> {
                val p = producer ?: return result.success(null)
                p.setSize(call.argument<Int>("width")!!, call.argument<Int>("height")!!)
                // A resize can replace the producer's Surface, so Bevy restarts on the new one.
                startBevy(p)
                result.success(null)
            }
            "touch" -> {
                BevyBridge.nativeTouch(
                    (call.argument<Number>("id")!!).toLong(),
                    call.argument<Int>("phase")!!,
                    (call.argument<Double>("x")!!).toFloat(),
                    (call.argument<Double>("y")!!).toFloat(),
                )
                result.success(null)
            }
            "command" -> {
                try {
                    BevyBridge.nativeCommand(call.argument<String>("json")!!)
                    result.success(null)
                } catch (e: IllegalArgumentException) {
                    result.error("bad_command", e.message, null)
                }
            }
            "stop" -> {
                stop()
                result.success(null)
            }
            "launchArguments" -> result.success(launchArguments)
            else -> result.notImplemented()
        }
    }

    private fun start(width: Int, height: Int): Long {
        stop()
        val p = textures.createSurfaceProducer(TextureRegistry.SurfaceLifecycle.manual)
        p.setSize(width, height)
        p.setCallback(object : TextureRegistry.SurfaceProducer.Callback {
            override fun onSurfaceAvailable() = startBevy(p)
            override fun onSurfaceCleanup() = BevyBridge.nativeStop()
        })
        producer = p
        startBevy(p)
        return p.id()
    }

    private fun startBevy(p: TextureRegistry.SurfaceProducer) {
        BevyBridge.nativeStart(p.surface, p.width, p.height, scale, scene) { json ->
            // Logged so CI's emulator smoke test can see the renderer come up.
            Log.i("BevyFlutter", json)
            main.post { sink?.success(json) }
        }
    }

    private fun stop() {
        BevyBridge.nativeStop()
        producer?.release()
        producer = null
    }

    override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
        sink = events
    }

    override fun onCancel(arguments: Any?) {
        sink = null
    }
}
