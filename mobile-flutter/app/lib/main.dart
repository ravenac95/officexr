import 'package:material_ui/material_ui.dart';

import 'join_screen.dart';

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
      home: const JoinScreen(),
    );
  }
}
