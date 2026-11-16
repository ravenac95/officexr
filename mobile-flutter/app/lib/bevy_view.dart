import 'dart:async';
import 'dart:convert';

import 'package:bevy_host/bevy_host.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// An event from the Bevy host (see `HostEvent` in `native/src/scene.rs`).
sealed class BevyEvent {
  static BevyEvent parse(String json) {
    final map = jsonDecode(json) as Map<String, dynamic>;
    return switch (map['type']) {
      'ready' => BevyReady(map['adapter'] as String),
      'stats' => BevyStats(
          (map['fps'] as num).toDouble(),
          map['cubes'] as int,
        ),
      'room' => BevyRoom(
          (map['fps'] as num).toDouble(),
          joined: map['joined'] as bool,
          peers: map['peers'] as int,
        ),
      'closed' => BevyClosed(map['reason'] as String),
      'error' => BevyError(map['message'] as String),
      final other => BevyError('unknown event $other'),
    };
  }
}

class BevyReady extends BevyEvent {
  BevyReady(this.adapter);
  final String adapter;
}

class BevyStats extends BevyEvent {
  BevyStats(this.fps, this.cubes);
  final double fps;
  final int cubes;
}

/// An office room's state, every half second.
class BevyRoom extends BevyEvent {
  BevyRoom(this.fps, {required this.joined, required this.peers});
  final double fps;
  final bool joined;
  final int peers;
}

/// The office's realtime connection gave up.
class BevyClosed extends BevyEvent {
  BevyClosed(this.reason);
  final String reason;
}

class BevyError extends BevyEvent {
  BevyError(this.message);
  final String message;
}

/// What Bevy shows: `SceneSpec` in `native/src/host.rs`.
sealed class BevyScene {
  const BevyScene();

  String toJson();
}

/// The spinning-cubes demo; needs no server.
class DemoScene extends BevyScene {
  const DemoScene();

  @override
  String toJson() => jsonEncode({'type': 'demo'});
}

/// A live office room.
class OfficeScene extends BevyScene {
  const OfficeScene({
    required this.webUrl,
    required this.realtimeUrl,
    required this.officeId,
    required this.sessionCookie,
  });

  /// Serves `/api/*` and `/models/*`, e.g. `https://officexr-api.example.com`.
  final String webUrl;

  /// The realtime WebSocket, e.g. `wss://officexr-api.example.com/ws`.
  final String realtimeUrl;
  final String officeId;

  /// The `Cookie` header of a signed-in browser session.
  final String sessionCookie;

  @override
  String toJson() => jsonEncode({
        'type': 'office',
        'config': {
          'web_url': webUrl,
          'realtime_url': realtimeUrl,
          'office_id': officeId,
          'session_cookie': sessionCookie,
        },
      });
}

/// Talks to the platform side (Kotlin `BevyTexture`, Swift `BevyHostPlugin`) over channels.
class BevyController {
  static const _methods = MethodChannel('officexr/bevy');
  static const _events = EventChannel('officexr/bevy/events');

  final Stream<BevyEvent> events = _events
      .receiveBroadcastStream()
      .map((e) => BevyEvent.parse(e as String))
      .asBroadcastStream();

  /// Starts Bevy showing `scene` on a new texture of `physical` pixels (Android only; iOS
  /// starts from the platform view); returns the texture id.
  Future<int> start(BevyScene scene, Size physical, double scale) async {
    final id = await _methods.invokeMethod<int>('start', {
      'width': physical.width.round(),
      'height': physical.height.round(),
      'scale': scale,
      'scene': scene.toJson(),
    });
    return id!;
  }

  Future<void> resize(Size physical) => _methods.invokeMethod('resize', {
        'width': physical.width.round(),
        'height': physical.height.round(),
      });

  Future<void> stop() => _methods.invokeMethod('stop');

  Future<void> command(Map<String, Object?> command) =>
      _methods.invokeMethod('command', {'json': jsonEncode(command)});

  void touch(int id, int phase, Offset position) {
    _methods.invokeMethod('touch', {
      'id': id,
      'phase': phase,
      'x': position.dx,
      'y': position.dy,
    });
  }
}

/// Shows the Bevy scene sized to this widget.
///
/// On Android, Bevy renders into a Flutter texture and this widget forwards touches to it. On
/// iOS, Bevy renders into a native Metal view (wgpu only creates Metal surfaces on the main
/// thread, which a texture's producer doesn't run on), embedded as a platform view that
/// handles its own touches.
class BevyView extends StatefulWidget {
  const BevyView({super.key, required this.controller, required this.scene});

  final BevyController controller;
  final BevyScene scene;

  @override
  State<BevyView> createState() => _BevyViewState();
}

class _BevyViewState extends State<BevyView> {
  int? _textureId;
  Size? _physical;
  Timer? _resizeDebounce;

  @override
  void dispose() {
    _resizeDebounce?.cancel();
    widget.controller.stop();
    super.dispose();
  }

  void _sizeChanged(Size logical, double scale) {
    final physical = logical * scale;
    if (physical == _physical || physical.isEmpty) {
      return;
    }
    final first = _physical == null;
    _physical = physical;
    if (first) {
      widget.controller.start(widget.scene, physical, scale).then((id) {
        if (mounted) {
          setState(() => _textureId = id);
        }
      });
    } else {
      // Resizing restarts Bevy on a new surface, so wait for the layout to settle.
      _resizeDebounce?.cancel();
      _resizeDebounce = Timer(const Duration(milliseconds: 250), () {
        widget.controller.resize(physical);
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    if (defaultTargetPlatform == TargetPlatform.iOS) {
      return UiKitView(
        viewType: bevyViewType,
        creationParams: {'scene': widget.scene.toJson()},
        creationParamsCodec: const StandardMessageCodec(),
        gestureRecognizers: {
          Factory<OneSequenceGestureRecognizer>(EagerGestureRecognizer.new),
        },
      );
    }
    final scale = MediaQuery.devicePixelRatioOf(context);
    return LayoutBuilder(builder: (context, constraints) {
      final size = constraints.biggest;
      WidgetsBinding.instance.addPostFrameCallback((_) => _sizeChanged(size, scale));
      final id = _textureId;
      return Listener(
        behavior: HitTestBehavior.opaque,
        onPointerDown: (e) => widget.controller.touch(e.pointer, 0, e.localPosition),
        onPointerMove: (e) => widget.controller.touch(e.pointer, 1, e.localPosition),
        onPointerUp: (e) => widget.controller.touch(e.pointer, 2, e.localPosition),
        onPointerCancel: (e) => widget.controller.touch(e.pointer, 3, e.localPosition),
        child: id == null ? const SizedBox.expand() : Texture(textureId: id),
      );
    });
  }
}
