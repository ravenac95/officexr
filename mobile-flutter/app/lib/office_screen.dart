import 'dart:async';

import 'package:material_ui/material_ui.dart';

import 'bevy_view.dart';
import 'glass.dart';

/// A live office full-bleed. Bevy draws its own touch controls (the walk stick bottom-left,
/// drag anywhere else to look), so Flutter only adds status at the top.
class OfficeScreen extends StatefulWidget {
  const OfficeScreen({super.key, required this.office});

  final OfficeScene office;

  @override
  State<OfficeScreen> createState() => _OfficeScreenState();
}

class _OfficeScreenState extends State<OfficeScreen> {
  final _bevy = BevyController();
  late final StreamSubscription<BevyEvent> _events;
  String _status = 'Connecting…';
  BevyRoom? _room;
  String? _error;

  @override
  void initState() {
    super.initState();
    _events = _bevy.events.listen((event) {
      setState(() {
        switch (event) {
          case BevyReady():
            _status = 'Joining…';
          case final BevyRoom room:
            _room = room;
            _status = room.joined ? 'In the office' : 'Joining…';
          case BevyClosed(:final reason):
            _error = 'Disconnected: $reason';
          case BevyError(:final message):
            _error = message;
          case BevyStats():
            break;
        }
      });
    });
  }

  @override
  void dispose() {
    _events.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final room = _room;
    final error = _error;
    return Scaffold(
      body: Stack(
        children: [
          Positioned.fill(child: BevyView(controller: _bevy, scene: widget.office)),
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Glass(
                child: Row(
                  children: [
                    IconButton(
                      tooltip: 'Leave',
                      onPressed: () => Navigator.of(context).pop(),
                      icon: const Icon(Icons.arrow_back),
                    ),
                    const SizedBox(width: 4),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(_status, style: theme.textTheme.titleMedium),
                          if (room != null)
                            Text(
                              '${room.peers} other${room.peers == 1 ? '' : 's'} here · '
                              '${room.fps.toStringAsFixed(0)} fps',
                              style: theme.textTheme.bodySmall,
                            ),
                          if (error != null)
                            Text(
                              error,
                              style: theme.textTheme.bodySmall
                                  ?.copyWith(color: theme.colorScheme.error),
                            ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
