//! The view-scoped controls API: TypeScript's frame loop takes each world
//! view's movement intent, steps the sim, and hands the player's new position
//! back as the view's follow target.

use std::sync::OnceLock;

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use officexr_controls::{CameraConfig, CameraMode, Controls, ControlsHub, ControlsPlugin};
use officexr_render::views::ViewId;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use super::{js_err, status};

static HUB: OnceLock<ControlsHub> = OnceLock::new();

#[wasm_bindgen]
extern "C" {
    type Document;
    #[wasm_bindgen(thread_local_v2, js_name = document)]
    static DOCUMENT: Document;
    #[wasm_bindgen(method, getter, js_name = pointerLockElement)]
    fn pointer_lock_element(this: &Document) -> JsValue;
}

pub(super) fn add(app: &mut App) {
    app.add_plugins(ControlsPlugin)
        .add_systems(PreUpdate, forget_lost_lock);
    let _ = HUB.set(app.world().resource::<ControlsHub>().clone());
}

/// The browser frees the pointer on Esc or a tab switch without telling Bevy,
/// which would otherwise keep turning the view on plain mouse moves.
fn forget_lost_lock(mut cursors: Query<&mut CursorOptions>) {
    let mut locked = cursors
        .iter_mut()
        .filter(|c| c.grab_mode != CursorGrabMode::None)
        .peekable();
    if locked.peek().is_none() || !DOCUMENT.with(Document::pointer_lock_element).is_null() {
        return;
    }
    for mut cursor in locked {
        // Unchanged as far as Bevy knows, so it doesn't exit a lock that's gone.
        let cursor = cursor.bypass_change_detection();
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

/// Runs `f` on a live view's controls.
fn with<R>(id: ViewId, f: impl FnOnce(&mut Controls) -> R) -> Result<R, JsError> {
    status(id)?;
    let hub = HUB.get().ok_or_else(|| js_err("call init first"))?;
    Ok(hub.with(id, f))
}

pub(super) fn forget(id: ViewId) {
    if let Some(hub) = HUB.get() {
        hub.remove(id);
    }
}

fn from_js<T: serde::de::DeserializeOwned>(value: JsValue) -> Result<T, JsError> {
    serde_wasm_bindgen::from_value(value).map_err(js_err)
}

#[derive(Serialize)]
struct Intent {
    dir: [f32; 2],
    run: bool,
    jump: bool,
    yaw: f32,
}

/// The view's movement since the last call: a world-space XZ `dir` (unit or
/// zero), `run`, `jump` (once per press) and the camera `yaw` it was turned by.
#[wasm_bindgen(js_name = takeIntent)]
pub fn take_intent(id: ViewId) -> Result<JsValue, JsError> {
    let intent = with(id, |c| {
        let intent = c.take_intent();
        Intent {
            dir: intent.dir,
            run: intent.run,
            jump: intent.jump,
            yaw: c.camera_yaw(),
        }
    })?;
    serde_wasm_bindgen::to_value(&intent).map_err(js_err)
}

/// The local player's body root `[x, y, z]` and yaw, for the camera to follow.
#[wasm_bindgen(js_name = setFollowTarget)]
pub fn set_follow_target(id: ViewId, pos: JsValue, yaw: f32) -> Result<(), JsError> {
    let pos: [f32; 3] = from_js(pos)?;
    with(id, |c| c.set_follow(pos.into(), yaw))
}

#[wasm_bindgen(js_name = setCameraMode)]
pub fn set_camera_mode(id: ViewId, mode: JsValue) -> Result<(), JsError> {
    let mode: CameraMode = from_js(mode)?;
    with(id, |c| c.mode = mode)
}

/// The conversation's centre `[x, y, z]`, or `null` outside one.
#[wasm_bindgen(js_name = setConversationFocus)]
pub fn set_conversation_focus(id: ViewId, point: JsValue) -> Result<(), JsError> {
    let point: Option<[f32; 3]> = from_js(point)?;
    with(id, |c| c.focus = point.map(Vec3::from))
}

/// Replaces the camera settings; a field left out takes its default.
#[wasm_bindgen(js_name = setCameraConfig)]
pub fn set_camera_config(id: ViewId, config: JsValue) -> Result<(), JsError> {
    let config: CameraConfig = from_js(config)?;
    with(id, |c| c.config = config)
}

/// Off while the page has keyboard focus elsewhere (a panel, a text field).
#[wasm_bindgen(js_name = setInputEnabled)]
pub fn set_input_enabled(id: ViewId, enabled: bool) -> Result<(), JsError> {
    with(id, |c| c.input_enabled = enabled)
}
