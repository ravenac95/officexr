import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import 'bevy_view.dart';

/// Remembers the last office joined (Keychain on iOS, encrypted storage on Android): the session
/// cookie is a credential, so it never goes to plain preferences.
class OfficeStore {
  OfficeStore([FlutterSecureStorage? storage])
      : _storage = storage ?? const FlutterSecureStorage();

  final FlutterSecureStorage _storage;

  static const _webUrl = 'web_url';
  static const _realtimeUrl = 'realtime_url';
  static const _officeId = 'office_id';
  static const _cookie = 'session_cookie';

  Future<OfficeScene?> load() async {
    final values = await _storage.readAll();
    final webUrl = values[_webUrl];
    final realtimeUrl = values[_realtimeUrl];
    final officeId = values[_officeId];
    final cookie = values[_cookie];
    if (webUrl == null || realtimeUrl == null || officeId == null || cookie == null) {
      return null;
    }
    return OfficeScene(
      webUrl: webUrl,
      realtimeUrl: realtimeUrl,
      officeId: officeId,
      sessionCookie: cookie,
    );
  }

  Future<void> save(OfficeScene office) async {
    await _storage.write(key: _webUrl, value: office.webUrl);
    await _storage.write(key: _realtimeUrl, value: office.realtimeUrl);
    await _storage.write(key: _officeId, value: office.officeId);
    await _storage.write(key: _cookie, value: office.sessionCookie);
  }
}
