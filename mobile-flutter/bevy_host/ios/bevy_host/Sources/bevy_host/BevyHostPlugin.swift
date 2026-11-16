import BevyHostCore
import Flutter
import UIKit

/// Registers the `officexr/bevy-view` platform view and the channels the app uses:
/// `officexr/bevy` (`command {json}`, `stop`) and `officexr/bevy/events` (host JSON events).
public final class BevyHostPlugin: NSObject, FlutterPlugin, FlutterStreamHandler {
    static var eventSink: FlutterEventSink?

    public static func register(with registrar: FlutterPluginRegistrar) {
        let instance = BevyHostPlugin()
        let methods = FlutterMethodChannel(
            name: "officexr/bevy", binaryMessenger: registrar.messenger())
        registrar.addMethodCallDelegate(instance, channel: methods)
        FlutterEventChannel(name: "officexr/bevy/events", binaryMessenger: registrar.messenger())
            .setStreamHandler(instance)
        registrar.register(BevyViewFactory(), withId: "officexr/bevy-view")
    }

    public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
        switch call.method {
        case "command":
            guard let args = call.arguments as? [String: Any], let json = args["json"] as? String
            else {
                result(FlutterError(code: "bad_args", message: "command needs json", details: nil))
                return
            }
            if json.withCString({ bevy_host_command($0) }) {
                result(nil)
            } else {
                result(FlutterError(code: "bad_command", message: json, details: nil))
            }
        case "stop":
            BevyMetalView.active?.stopBevy()
            result(nil)
        default:
            result(FlutterMethodNotImplemented)
        }
    }

    public func onListen(withArguments arguments: Any?, eventSink events: @escaping FlutterEventSink)
        -> FlutterError?
    {
        BevyHostPlugin.eventSink = events
        return nil
    }

    public func onCancel(withArguments arguments: Any?) -> FlutterError? {
        BevyHostPlugin.eventSink = nil
        return nil
    }
}

/// Called by Rust on the main thread, inside `bevy_host_frame`/`bevy_host_start`.
func bevyHostEvent(_ context: UnsafeMutableRawPointer?, _ json: UnsafePointer<CChar>?) {
    guard let json else { return }
    let text = String(cString: json)
    // Printed so CI's simulator smoke test can see the renderer come up.
    print("BevyFlutter \(text)")
    BevyHostPlugin.eventSink?(text)
}

final class BevyViewFactory: NSObject, FlutterPlatformViewFactory {
    func create(withFrame frame: CGRect, viewIdentifier viewId: Int64, arguments args: Any?)
        -> FlutterPlatformView
    {
        BevyPlatformView(frame: frame)
    }
}

final class BevyPlatformView: NSObject, FlutterPlatformView {
    private let metalView: BevyMetalView

    init(frame: CGRect) {
        metalView = BevyMetalView(frame: frame)
    }

    func view() -> UIView { metalView }
}
