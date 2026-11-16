package dev.officexr.bevy_flutter

import dev.officexr.bevyflutter.BevyTexture
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        // `adb shell am start … --esa officexr_args demo` opens the demo directly (CI does).
        val args = intent?.getStringArrayExtra("officexr_args")?.toList() ?: emptyList()
        BevyTexture(flutterEngine.dartExecutor.binaryMessenger, flutterEngine.renderer, args)
    }
}
