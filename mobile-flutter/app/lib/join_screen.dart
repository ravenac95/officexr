import 'package:material_ui/material_ui.dart';

import 'bevy_view.dart';
import 'demo_screen.dart';
import 'launch.dart';
import 'office_screen.dart';
import 'office_store.dart';

/// Where to go: a live office (server, office id and a session cookie pasted from a signed-in
/// browser), or the demo scene, which needs no server.
class JoinScreen extends StatefulWidget {
  const JoinScreen({super.key, this.store});

  final OfficeStore? store;

  @override
  State<JoinScreen> createState() => _JoinScreenState();
}

class _JoinScreenState extends State<JoinScreen> {
  late final OfficeStore _store = widget.store ?? OfficeStore();
  final _form = GlobalKey<FormState>();
  final _webUrl = TextEditingController();
  final _realtimeUrl = TextEditingController();
  final _officeId = TextEditingController();
  final _cookie = TextEditingController();
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _restore();
  }

  Future<void> _restore() async {
    final saved = await _store.load();
    if (saved != null) {
      _webUrl.text = saved.webUrl;
      _realtimeUrl.text = saved.realtimeUrl;
      _officeId.text = saved.officeId;
      _cookie.text = saved.sessionCookie;
    }
    final args = await launchArguments();
    if (!mounted) {
      return;
    }
    setState(() => _loading = false);
    // CI's smoke tests launch straight into the demo.
    if (args.contains('demo')) {
      _openDemo();
    }
  }

  @override
  void dispose() {
    for (final c in [_webUrl, _realtimeUrl, _officeId, _cookie]) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _join() async {
    if (!_form.currentState!.validate()) {
      return;
    }
    final office = OfficeScene(
      webUrl: _webUrl.text.trim().replaceAll(RegExp(r'/+$'), ''),
      realtimeUrl: _realtimeUrl.text.trim(),
      officeId: _officeId.text.trim(),
      sessionCookie: _cookie.text.trim(),
    );
    await _store.save(office);
    if (!mounted) {
      return;
    }
    await Navigator.of(context).push(
      MaterialPageRoute<void>(builder: (_) => OfficeScreen(office: office)),
    );
  }

  void _openDemo() {
    Navigator.of(context).push(
      MaterialPageRoute<void>(builder: (_) => const DemoScreen()),
    );
  }

  String? _required(String? value) =>
      (value == null || value.trim().isEmpty) ? 'Required' : null;

  String? _url(String? value, List<String> schemes) {
    final missing = _required(value);
    if (missing != null) {
      return missing;
    }
    final uri = Uri.tryParse(value!.trim());
    return uri != null && schemes.contains(uri.scheme) && uri.host.isNotEmpty
        ? null
        : 'Must start with ${schemes.join(' or ')}://';
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      appBar: AppBar(title: const Text('OfficeXR')),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : Form(
              key: _form,
              child: ListView(
                padding: const EdgeInsets.all(16),
                children: [
                  Text('Join an office', style: theme.textTheme.titleLarge),
                  const SizedBox(height: 4),
                  Text(
                    'Sign in on the web, open the office, and copy its id from the URL and the '
                    'Cookie header of any /api/me request from your browser\'s dev tools.',
                    style: theme.textTheme.bodySmall,
                  ),
                  const SizedBox(height: 16),
                  TextFormField(
                    controller: _webUrl,
                    decoration: const InputDecoration(
                      labelText: 'Server URL',
                      hintText: 'https://officexr-api.example.com',
                    ),
                    keyboardType: TextInputType.url,
                    autocorrect: false,
                    validator: (v) => _url(v, ['http', 'https']),
                  ),
                  TextFormField(
                    controller: _realtimeUrl,
                    decoration: const InputDecoration(
                      labelText: 'Realtime URL',
                      hintText: 'wss://officexr-api.example.com/ws',
                    ),
                    keyboardType: TextInputType.url,
                    autocorrect: false,
                    validator: (v) => _url(v, ['ws', 'wss']),
                  ),
                  TextFormField(
                    controller: _officeId,
                    decoration: const InputDecoration(labelText: 'Office id'),
                    autocorrect: false,
                    validator: _required,
                  ),
                  TextFormField(
                    controller: _cookie,
                    decoration: const InputDecoration(
                      labelText: 'Session cookie',
                      helperText: 'Stored in the Keychain / encrypted storage on this device.',
                    ),
                    obscureText: true,
                    autocorrect: false,
                    enableSuggestions: false,
                    validator: _required,
                  ),
                  const SizedBox(height: 24),
                  FilledButton.icon(
                    onPressed: _join,
                    icon: const Icon(Icons.login),
                    label: const Text('Join office'),
                  ),
                  const SizedBox(height: 8),
                  OutlinedButton.icon(
                    onPressed: _openDemo,
                    icon: const Icon(Icons.view_in_ar),
                    label: const Text('Open the demo scene'),
                  ),
                ],
              ),
            ),
    );
  }
}
