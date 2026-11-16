import 'package:flutter/services.dart';

/// The arguments the app was launched with: Android intent extra `officexr_args`, iOS process
/// arguments. CI passes `demo` to open the demo scene without tapping.
Future<List<String>> launchArguments() async {
  try {
    final args = await const MethodChannel('officexr/bevy')
        .invokeListMethod<String>('launchArguments');
    return args ?? const [];
  } on MissingPluginException {
    return const [];
  }
}
