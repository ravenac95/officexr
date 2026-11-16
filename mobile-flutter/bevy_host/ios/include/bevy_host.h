// C API of libbevy_flutter_host (mobile-flutter/native/src/ios.rs). Main thread only.
#pragma once

#include <stdbool.h>
#include <stdint.h>

/// Receives each host event as NUL-terminated JSON, valid only for the call.
typedef void (*BevyEventCallback)(void *context, const char *json);

/// Starts Bevy on `ui_view` (a UIView whose layer is a CAMetalLayer), replacing any running
/// host. `width`/`height` are physical pixels. The view, callback and context must stay valid
/// until `bevy_host_stop`.
bool bevy_host_start(void *ui_view, uint32_t width, uint32_t height, float scale,
                     BevyEventCallback callback, void *context);

/// Runs one frame and delivers its events. Call from a CADisplayLink.
void bevy_host_frame(void);

/// `phase`: 0 began, 1 moved, 2 ended, 3 cancelled. Positions in points.
void bevy_host_touch(uint64_t id, int32_t phase, float x, float y);

void bevy_host_resize(uint32_t width, uint32_t height);

/// Applies a SceneCommand given as JSON; false if it doesn't parse.
bool bevy_host_command(const char *json);

void bevy_host_stop(void);
