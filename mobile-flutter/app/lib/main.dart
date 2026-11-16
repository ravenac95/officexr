import 'dart:async';

import 'package:material_ui/material_ui.dart';

import 'bevy_view.dart';

void main() {
  runApp(const BevyFlutterApp());
}

class BevyFlutterApp extends StatelessWidget {
  const BevyFlutterApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Bevy + Flutter',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF4088FF),
          brightness: Brightness.dark,
        ),
      ),
      home: const RoomScreen(),
    );
  }
}

/// The Bevy scene full-bleed, with Flutter widgets drawn over it.
class RoomScreen extends StatefulWidget {
  const RoomScreen({super.key});

  @override
  State<RoomScreen> createState() => _RoomScreenState();
}

class _RoomScreenState extends State<RoomScreen> {
  final _bevy = BevyController();
  late final StreamSubscription<BevyEvent> _events;
  String _adapter = 'starting Bevy…';
  BevyStats? _stats;
  String? _error;
  double _speed = 0.25;
  int _color = 0;

  static const _colors = [
    Color(0xFF4088FF),
    Color(0xFFFF6B4A),
    Color(0xFF3DDC97),
    Color(0xFFFFC145),
    Color(0xFFB57BFF),
  ];

  @override
  void initState() {
    super.initState();
    _events = _bevy.events.listen((event) {
      setState(() {
        switch (event) {
          case BevyReady(:final adapter):
            _adapter = adapter;
          case final BevyStats stats:
            _stats = stats;
          case BevyError(:final message):
            _error = message;
        }
      });
    });
  }

  @override
  void dispose() {
    _events.cancel();
    super.dispose();
  }

  void _setColor(int index) {
    setState(() => _color = index);
    final c = _colors[index];
    _bevy.command({'type': 'setColor', 'r': c.r, 'g': c.g, 'b': c.b});
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final stats = _stats;
    return Scaffold(
      body: Stack(
        children: [
          Positioned.fill(child: BevyView(controller: _bevy)),
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _Glass(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Bevy inside Flutter', style: theme.textTheme.titleMedium),
                        const SizedBox(height: 4),
                        Text(_adapter, style: theme.textTheme.bodySmall),
                        if (stats != null)
                          Text(
                            '${stats.fps.toStringAsFixed(0)} fps · ${stats.cubes} cubes',
                            style: theme.textTheme.bodySmall,
                          ),
                        if (_error != null)
                          Text(
                            _error!,
                            style: theme.textTheme.bodySmall
                                ?.copyWith(color: theme.colorScheme.error),
                          ),
                      ],
                    ),
                  ),
                  const Spacer(),
                  _Glass(child: _controls(context)),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _controls(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            const Icon(Icons.speed, size: 20),
            Expanded(
              child: Slider(
                min: -2,
                max: 2,
                value: _speed,
                onChanged: (v) {
                  setState(() => _speed = v);
                  _bevy.command({'type': 'setSpeed', 'turnsPerSecond': v});
                },
              ),
            ),
          ],
        ),
        Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            for (var i = 0; i < _colors.length; i++)
              GestureDetector(
                onTap: () => _setColor(i),
                child: Container(
                  width: 32,
                  height: 32,
                  decoration: BoxDecoration(
                    color: _colors[i],
                    shape: BoxShape.circle,
                    border: Border.all(
                      color: Colors.white,
                      width: i == _color ? 3 : 0,
                    ),
                  ),
                ),
              ),
          ],
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            IconButton.filledTonal(
              tooltip: 'Remove cube',
              onPressed: () => _bevy.command({'type': 'removeCube'}),
              icon: const Icon(Icons.remove),
            ),
            IconButton.filledTonal(
              tooltip: 'Add cube',
              onPressed: () => _bevy.command({'type': 'addCube'}),
              icon: const Icon(Icons.add),
            ),
            IconButton.filledTonal(
              tooltip: 'Reset camera',
              onPressed: () => _bevy.command({'type': 'resetCamera'}),
              icon: const Icon(Icons.center_focus_strong),
            ),
            const Spacer(),
            FilledButton.icon(
              onPressed: () => Navigator.of(context).push(
                MaterialPageRoute<void>(builder: (_) => const AboutScreen()),
              ),
              icon: const Icon(Icons.info_outline),
              label: const Text('Flutter page'),
            ),
          ],
        ),
      ],
    );
  }
}

class _Glass extends StatelessWidget {
  const _Glass({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Colors.black.withValues(alpha: 0.55),
        borderRadius: BorderRadius.circular(16),
      ),
      child: child,
    );
  }
}

/// An ordinary Flutter screen, to show navigation on top of the 3D view.
class AboutScreen extends StatelessWidget {
  const AboutScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Plain Flutter screen')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: const [
          Text(
            'This page is all Flutter. The 3D view behind it is Bevy, rendered from Rust '
            'into a Flutter texture (a SurfaceProducer). Flutter owns the app; Bevy only '
            'draws the scene and takes touches and commands over a platform channel.',
          ),
          SizedBox(height: 16),
          ListTile(leading: Icon(Icons.touch_app), title: Text('One finger orbits')),
          ListTile(leading: Icon(Icons.pinch), title: Text('Two fingers zoom')),
          ListTile(
            leading: Icon(Icons.tune),
            title: Text('Speed, colour and cube count are Flutter widgets'),
          ),
        ],
      ),
    );
  }
}
