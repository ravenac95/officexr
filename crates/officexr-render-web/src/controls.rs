//! The view-scoped controls API: TypeScript's frame loop takes each world
//! view's movement intent, steps the sim, and hands the player's new position
//! back as the view's follow target.

mod page_input;

use std::sync::OnceLock;

use bevy::prelude::*;
use officexr_controls::motion::DeviceReading;
use officexr_controls::{
    CameraConfig, CameraMode, Controls, ControlsHub, ControlsPlugin, JoystickKind,
};
use officexr_render::views::ViewId;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use super::{js_err, status, to_json};

static HUB: OnceLock<ControlsHub> = OnceLock::new();

pub(super) fn add(app: &mut App) {
    app.add_plugins(ControlsPlugin);
    page_input::add(app);
    let _ = HUB.set(app.world().resource::<ControlsHub>().clone());
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

/// NaN or infinity would stick in the camera's pose for good.
fn finite(numbers: impl IntoIterator<Item = f32>) -> Result<(), JsError> {
    if numbers.into_iter().all(f32::is_finite) {
        Ok(())
    } else {
        Err(js_err("numbers must be finite"))
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
    deflection: f32,
    yaw: f32,
}

/// The view's movement since the last call: a world-space XZ `dir` (unit or
/// zero), `run`, `jump` (once per press), the stick's `deflection` when it gave `dir` (else 0)
/// and the camera `yaw` it was turned by.
#[wasm_bindgen(js_name = takeIntent)]
pub fn take_intent(id: ViewId) -> Result<JsValue, JsError> {
    let intent = with(id, |c| {
        let intent = c.take_intent();
        Intent {
            dir: intent.dir,
            run: intent.run,
            jump: intent.jump,
            deflection: intent.deflection,
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

/// Replaces the camera settings; a field left out (or `undefined`) takes its
/// default, and an unknown field is an error.
#[wasm_bindgen(js_name = setCameraConfig)]
pub fn set_camera_config(id: ViewId, config: JsValue) -> Result<(), JsError> {
    let json = to_json(&config).ok_or_else(|| js_err("camera config must be an object"))?;
    let config: CameraConfig = serde_json::from_str(&json).map_err(js_err)?;
    with(id, |c| c.config = config)
}

/// Fields left out keep their current values.
#[derive(Deserialize)]
struct OrbitPose {
    target: Option<[f32; 3]>,
    azimuth: Option<f32>,
    elevation: Option<f32>,
    distance: Option<f32>,
}

/// Moves the orbit camera at once, within the config's limits.
#[wasm_bindgen(js_name = setOrbit)]
pub fn set_orbit(id: ViewId, pose: JsValue) -> Result<(), JsError> {
    let pose: OrbitPose = from_js(pose)?;
    let numbers = [pose.azimuth, pose.elevation, pose.distance];
    finite(
        pose.target
            .into_iter()
            .flatten()
            .chain(numbers.into_iter().flatten()),
    )?;
    with(id, |c| {
        c.set_orbit(|orbit| {
            orbit.target = pose.target.map_or(orbit.target, Vec3::from);
            orbit.azimuth = pose.azimuth.unwrap_or(orbit.azimuth);
            orbit.elevation = pose.elevation.unwrap_or(orbit.elevation);
            orbit.distance = pose.distance.unwrap_or(orbit.distance);
        });
    })
}

#[derive(Deserialize)]
struct Aabb {
    min: [f32; 3],
    max: [f32; 3],
}

/// Fits the orbit camera to `{ min, max }`, keeping its angles.
#[wasm_bindgen(js_name = frameBounds)]
pub fn frame_bounds(id: ViewId, aabb: JsValue) -> Result<(), JsError> {
    let Aabb { min, max } = from_js(aabb)?;
    finite(min.into_iter().chain(max))?;
    with(id, |c| c.frame_bounds(min.into(), max.into()))
}

/// Off while the page has keyboard focus elsewhere (a panel, a text field).
#[wasm_bindgen(js_name = setInputEnabled)]
pub fn set_input_enabled(id: ViewId, enabled: bool) -> Result<(), JsError> {
    with(id, |c| c.input_enabled = enabled)
}

/// The host's on-screen stick `[x, y]`: `x` strafe (right +), `y` forward (+), within the unit
/// circle. Held movement keys win over it.
#[wasm_bindgen(js_name = setStick)]
pub fn set_stick(id: ViewId, stick: JsValue) -> Result<(), JsError> {
    let stick: [f32; 2] = from_js(stick)?;
    finite(stick)?;
    with(id, |c| c.stick = Vec2::from(stick).clamp_length_max(1.0))
}

/// A `deviceorientation` event's `{ alpha, beta, gamma }`; nulls forget the previous reading.
#[wasm_bindgen(js_name = setDeviceOrientation)]
pub fn set_device_orientation(id: ViewId, reading: JsValue) -> Result<(), JsError> {
    let reading: DeviceReading = from_js(reading)?;
    let angles = [reading.alpha, reading.beta, reading.gamma];
    if !angles.into_iter().flatten().all(f64::is_finite) {
        return Err(js_err("numbers must be finite"));
    }
    with(id, |c| c.orient(reading))
}

/// `'bevy'` shows the view's own touch stick; `'dom'` hides it for the host's, fed by `setStick`.
#[wasm_bindgen(js_name = setJoystickKind)]
pub fn set_joystick_kind(id: ViewId, kind: JsValue) -> Result<(), JsError> {
    let kind: JoystickKind = from_js(kind)?;
    with(id, |c| {
        c.joystick = kind;
        c.stick = Vec2::ZERO;
    })
}
