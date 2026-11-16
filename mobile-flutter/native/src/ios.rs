//! C entry points for the iOS Flutter plugin (`mobile-flutter/bevy_host/ios`).
//!
//! Every function must be called on the main thread. wgpu only creates a Metal surface from a
//! `UIView` there, so the plugin steps the [`Host`] from a `CADisplayLink` instead of Rust
//! running a loop of its own, and events are delivered synchronously inside
//! `bevy_host_frame`.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::NonNull;

use bevy::input::touch::TouchPhase;
use bevy::window::{RawHandleWrapper, WindowWrapper};
use crossbeam_channel::Receiver;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
    UiKitWindowHandle, WindowHandle,
};

use crate::events::HostEvent;
use crate::host::{Host, HostInput, SceneSpec, SurfaceTarget};
use crate::scene::SceneCommand;

/// Receives each event as a NUL-terminated JSON string, valid only for the call.
pub type EventCallback = extern "C" fn(context: *mut c_void, json: *const c_char);

struct Running {
    host: Host,
    events: Receiver<HostEvent>,
    callback: EventCallback,
    context: *mut c_void,
}

thread_local! {
    // Main-thread only, like the UIView it renders into.
    static RUNNING: RefCell<Option<Running>> = const { RefCell::new(None) };
}

/// A `UIView` the plugin keeps alive until `bevy_host_stop`.
struct UiView(NonNull<c_void>);

// SAFETY: the pointer is only dereferenced by wgpu's surface creation, which asserts it's on
// the main thread; the wrapper itself is just carried through Bevy's Send + Sync components.
unsafe impl Send for UiView {}
// SAFETY: as above.
unsafe impl Sync for UiView {}

impl HasWindowHandle for UiView {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::UiKit(UiKitWindowHandle::new(self.0));
        // SAFETY: the plugin guarantees the view outlives the host.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for UiView {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::uikit())
    }
}

/// Starts Bevy on `ui_view` (a `UIView` whose layer is a `CAMetalLayer`) showing `scene_json`
/// (a `SceneSpec`), replacing any running host. `width`/`height` are physical pixels. Returns
/// false if it couldn't start, after sending an `error` event saying why.
///
/// # Safety
/// `ui_view` must be a live `UIView` that stays alive until `bevy_host_stop` or the next
/// `bevy_host_start`; `scene_json` a valid NUL-terminated string; `callback` and `context` must
/// stay valid for as long as the view.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bevy_host_start(
    ui_view: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
    scene_json: *const c_char,
    callback: EventCallback,
    context: *mut c_void,
) -> bool {
    bevy_host_stop();
    let fail = |message: String| {
        send_now(callback, context, &HostEvent::Error { message });
        false
    };
    if scene_json.is_null() {
        return fail("no scene".into());
    }
    // SAFETY: the caller passes a valid C string.
    let scene = match unsafe { CStr::from_ptr(scene_json) }.to_str() {
        Ok(json) => SceneSpec::parse(json),
        Err(err) => Err(err.to_string()),
    };
    let scene = match scene {
        Ok(scene) => scene,
        Err(message) => return fail(message),
    };
    let Some(view) = NonNull::new(ui_view) else {
        return fail("no view".into());
    };
    let handle = match RawHandleWrapper::new(&WindowWrapper::new(UiView(view))) {
        Ok(handle) => handle,
        Err(err) => return fail(err.to_string()),
    };
    let (event_tx, event_rx) = crossbeam_channel::unbounded();
    let target = SurfaceTarget {
        handle,
        width: width.max(1),
        height: height.max(1),
        scale,
    };
    let host = match Host::new(target, scene, event_tx) {
        Ok(host) => host,
        Err(message) => return fail(message),
    };
    let running = Running {
        host,
        events: event_rx,
        callback,
        context,
    };
    deliver(&running);
    RUNNING.with_borrow_mut(|slot| *slot = Some(running));
    true
}

/// Runs one frame and delivers the events it produced. Call it from a `CADisplayLink`.
#[unsafe(no_mangle)]
pub extern "C" fn bevy_host_frame() {
    RUNNING.with_borrow_mut(|slot| {
        if let Some(running) = slot.as_mut() {
            running.host.frame();
            deliver(running);
        }
    });
}

fn deliver(running: &Running) {
    for event in running.events.try_iter() {
        send_now(running.callback, running.context, &event);
    }
}

fn send_now(callback: EventCallback, context: *mut c_void, event: &HostEvent) {
    let Ok(Ok(json)) = serde_json::to_string(event).map(CString::new) else {
        return;
    };
    callback(context, json.as_ptr());
}

fn apply(input: HostInput) {
    RUNNING.with_borrow_mut(|slot| {
        if let Some(running) = slot.as_mut() {
            running.host.apply(input);
        }
    });
}

/// `phase`: 0 began, 1 moved, 2 ended, 3 cancelled. Positions are points (logical pixels).
#[unsafe(no_mangle)]
pub extern "C" fn bevy_host_touch(id: u64, phase: i32, x: f32, y: f32) {
    let phase = match phase {
        0 => TouchPhase::Started,
        1 => TouchPhase::Moved,
        2 => TouchPhase::Ended,
        _ => TouchPhase::Canceled,
    };
    apply(HostInput::Touch { id, phase, x, y });
}

#[unsafe(no_mangle)]
pub extern "C" fn bevy_host_resize(width: u32, height: u32) {
    apply(HostInput::Resize {
        width: width.max(1),
        height: height.max(1),
    });
}

/// Applies a `SceneCommand` given as JSON. Returns false if it doesn't parse.
///
/// # Safety
/// `json` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bevy_host_command(json: *const c_char) -> bool {
    if json.is_null() {
        return false;
    }
    // SAFETY: the caller passes a valid C string.
    let json = unsafe { CStr::from_ptr(json) };
    match json
        .to_str()
        .ok()
        .and_then(|s| serde_json::from_str::<SceneCommand>(s).ok())
    {
        Some(command) => {
            apply(HostInput::Scene(command));
            true
        }
        None => false,
    }
}

/// Drops the running host, if any, releasing its surface.
#[unsafe(no_mangle)]
pub extern "C" fn bevy_host_stop() {
    let running = RUNNING.with_borrow_mut(Option::take);
    drop(running);
}
