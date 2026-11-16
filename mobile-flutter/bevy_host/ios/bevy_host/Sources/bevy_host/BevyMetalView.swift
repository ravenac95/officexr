import BevyHostCore
import QuartzCore
import UIKit

/// A view backed by a `CAMetalLayer` that Bevy renders into. Bevy starts once the view has a
/// size and a window, is stepped by a `CADisplayLink` on the main thread, and stops when the
/// view leaves its window. Touches go straight to Rust.
final class BevyMetalView: UIView {
    /// The host is a process-wide singleton on the Rust side, so only one view drives it.
    static weak var active: BevyMetalView?

    override class var layerClass: AnyClass { CAMetalLayer.self }

    private var displayLink: CADisplayLink?
    private var running = false
    private var runningSize: CGSize = .zero

    override init(frame: CGRect) {
        super.init(frame: frame)
        isMultipleTouchEnabled = true
        backgroundColor = .black
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) is not supported")
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil {
            stopBevy()
        } else {
            contentScaleFactor = window?.screen.scale ?? contentScaleFactor
            setNeedsLayout()
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard window != nil else { return }
        let scale = contentScaleFactor
        let width = UInt32(max(bounds.width * scale, 0))
        let height = UInt32(max(bounds.height * scale, 0))
        guard width > 0, height > 0 else { return }
        if !running {
            startBevy(width: width, height: height, scale: Float(scale))
        } else if bounds.size != runningSize {
            bevy_host_resize(width, height)
        }
        runningSize = bounds.size
    }

    private func startBevy(width: UInt32, height: UInt32, scale: Float) {
        BevyMetalView.active?.stopBevy()
        let view = Unmanaged.passUnretained(self).toOpaque()
        guard bevy_host_start(view, width, height, scale, bevyHostEvent, nil) else {
            print("BevyFlutter {\"type\":\"error\",\"message\":\"bevy_host_start failed\"}")
            return
        }
        running = true
        BevyMetalView.active = self
        let link = CADisplayLink(target: self, selector: #selector(step))
        link.add(to: .main, forMode: .common)
        displayLink = link
    }

    @objc private func step() {
        bevy_host_frame()
    }

    func stopBevy() {
        // The display link retains its target; invalidating it breaks that cycle.
        displayLink?.invalidate()
        displayLink = nil
        guard running else { return }
        running = false
        bevy_host_stop()
        if BevyMetalView.active === self {
            BevyMetalView.active = nil
        }
    }

    private func forward(_ touches: Set<UITouch>, phase: Int32) {
        guard running else { return }
        for touch in touches {
            let point = touch.location(in: self)
            let id = UInt64(UInt(bitPattern: ObjectIdentifier(touch).hashValue))
            bevy_host_touch(id, phase, Float(point.x), Float(point.y))
        }
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        forward(touches, phase: 0)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        forward(touches, phase: 1)
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        forward(touches, phase: 2)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        forward(touches, phase: 3)
    }
}
