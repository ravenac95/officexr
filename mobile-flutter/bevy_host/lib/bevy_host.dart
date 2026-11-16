/// The iOS plugin has no Dart API of its own: the app talks to it over the `officexr/bevy`
/// method channel and `officexr/bevy/events` event channel, and embeds the
/// `officexr/bevy-view` platform view (see the app's `bevy_view.dart`).
library;

/// The platform view type the plugin registers.
const bevyViewType = 'officexr/bevy-view';
