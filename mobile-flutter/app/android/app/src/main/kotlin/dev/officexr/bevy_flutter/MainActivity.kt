package dev.officexr.bevy_flutter

import dev.officexr.bevyflutter.BevyTexture
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        BevyTexture(flutterEngine.dartExecutor.binaryMessenger, flutterEngine.renderer)
    }
}
