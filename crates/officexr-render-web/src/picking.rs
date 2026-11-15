//! `onPick`, `rayAtScreen` and `intersectPlane`. Every view takes its
//! pointer from its own canvas's DOM events, whatever it draws into: a
//! WebGL2 image view's canvas never reaches Bevy.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use officexr_controls::{ControlsSystems, PointerClaimed};
use officexr_render::SceneCamera;
use officexr_render::picking::{
    Modifiers, PickHub, PointerMsg, PointerMsgAction, TargetPressed, intersect_plane as plane_hit,
    ray_at,
};
use officexr_render::views::{View, ViewCamera, ViewId};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use super::{Canvas, PRIMARY_ATTR, PRIMARY_VIEW, js_err, query_selector};

static PICKS: OnceLock<PickHub> = OnceLock::new();

#[wasm_bindgen]
extern "C" {
    type PointerEvent;
    #[wasm_bindgen(method, getter, js_name = offsetX)]
    fn offset_x(this: &PointerEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = offsetY)]
    fn offset_y(this: &PointerEvent) -> f64;
    #[wasm_bindgen(method, getter)]
    fn button(this: &PointerEvent) -> i16;
    #[wasm_bindgen(method, getter)]
    fn buttons(this: &PointerEvent) -> u16;
    #[wasm_bindgen(method, getter, js_name = pointerId)]
    fn pointer_id(this: &PointerEvent) -> i32;
    #[wasm_bindgen(method, getter, js_name = shiftKey)]
    fn shift_key(this: &PointerEvent) -> bool;
    #[wasm_bindgen(method, getter, js_name = ctrlKey)]
    fn ctrl_key(this: &PointerEvent) -> bool;
    #[wasm_bindgen(method, getter, js_name = altKey)]
    fn alt_key(this: &PointerEvent) -> bool;
    #[wasm_bindgen(method, getter, js_name = metaKey)]
    fn meta_key(this: &PointerEvent) -> bool;

    #[wasm_bindgen(method, getter, js_name = clientWidth)]
    fn css_width(this: &Canvas) -> f64;
    #[wasm_bindgen(method, getter, js_name = clientHeight)]
    fn css_height(this: &Canvas) -> f64;
    #[wasm_bindgen(method, js_name = addEventListener)]
    fn add_event_listener(this: &Canvas, kind: &str, listener: &Listener);
    #[wasm_bindgen(method, js_name = removeEventListener)]
    fn remove_event_listener(this: &Canvas, kind: &str, listener: &Listener);
    #[wasm_bindgen(method, js_name = setPointerCapture)]
    fn set_pointer_capture(this: &Canvas, id: i32);

    type PickCallback;
    /// Caught: a throw unwinding through Bevy's schedule would wedge the app.
    #[wasm_bindgen(method, catch, js_name = call)]
    fn call(this: &PickCallback, this_arg: &JsValue, event: &JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str, error: &JsValue);
}

type Listener = Closure<dyn FnMut(PointerEvent)>;

struct Subscription {
    canvas: Canvas,
    callback: PickCallback,
    listeners: Vec<(&'static str, Listener)>,
}

thread_local! {
    static SUBSCRIPTIONS: RefCell<HashMap<ViewId, Subscription>> = RefCell::default();
    /// Each view's camera as last drawn, or its `setCamera` since, for the
    /// synchronous ray queries: the controls may be driving it.
    static CAMERAS: RefCell<HashMap<ViewId, SceneCamera>> = RefCell::default();
}

pub(super) fn add(app: &mut App) {
    let _ = PICKS.set(app.world().resource::<PickHub>().clone());
    app.add_systems(Update, claim_pointer.before(ControlsSystems))
        .add_systems(Last, (mirror_cameras, deliver));
}

/// A primary press on a pickable target is the picker's: it takes no pointer lock.
fn claim_pointer(mut pressed: MessageReader<TargetPressed>, mut claimed: ResMut<PointerClaimed>) {
    if pressed.read().any(|p| p.button == PointerButton::Primary) {
        claimed.0 = true;
    }
}

fn mirror_cameras(cameras: Query<(&View, &ViewCamera), Changed<ViewCamera>>) {
    CAMERAS.with(|c| {
        let mut c = c.borrow_mut();
        for (view, camera) in &cameras {
            c.insert(view.id, camera.0.clone());
        }
    });
}

pub(super) fn set_camera(id: ViewId, camera: SceneCamera) {
    CAMERAS.with(|c| c.borrow_mut().insert(id, camera));
}

pub(super) fn forget(id: ViewId) {
    unsubscribe(id);
    CAMERAS.with(|c| c.borrow_mut().remove(&id));
}

fn view_canvas(id: ViewId) -> Result<Canvas, JsError> {
    let selector = if PRIMARY_VIEW.get() == Some(id) {
        format!("canvas[{PRIMARY_ATTR}]")
    } else {
        format!("canvas[data-officexr-view=\"{id}\"]")
    };
    query_selector(&selector)
        .dyn_into()
        .map_err(|_| js_err(format!("no view {id}")))
}

fn size(canvas: &Canvas) -> Vec2 {
    Vec2::new(canvas.css_width() as f32, canvas.css_height() as f32)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PickOptions {
    #[serde(default)]
    bell_claims_only: bool,
}

/// Sends `callback` the view's pick events, replacing any it had; `null` stops them.
#[wasm_bindgen(js_name = onPick)]
pub fn on_pick(id: ViewId, callback: JsValue, options: JsValue) -> Result<(), JsError> {
    unsubscribe(id);
    if callback.is_null() || callback.is_undefined() {
        return Ok(());
    }
    let options: PickOptions = if options.is_undefined() {
        PickOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options).map_err(js_err)?
    };
    let picks = PICKS.get().ok_or_else(|| js_err("call init first"))?;
    picks.claim_only_bells(id, options.bell_claims_only);
    let canvas = view_canvas(id)?;
    let listener = |action: fn(&PointerEvent) -> Option<PointerMsgAction>| {
        let canvas: Canvas = JsValue::clone(&canvas).unchecked_into();
        let picks = picks.clone();
        Listener::new(move |e: PointerEvent| {
            let Some(action) = action(&e) else {
                return;
            };
            if matches!(action, PointerMsgAction::Press(_)) {
                // Drags keep reporting past the canvas's edge.
                canvas.set_pointer_capture(e.pointer_id());
            }
            let modifiers = Modifiers {
                shift: e.shift_key(),
                ctrl: e.ctrl_key(),
                alt: e.alt_key(),
                meta: e.meta_key(),
            };
            let screen = Vec2::new(e.offset_x() as f32, e.offset_y() as f32);
            let size = size(&canvas);
            picks.pointer(
                id,
                PointerMsg {
                    action,
                    screen,
                    size,
                    modifiers,
                },
            );
        })
    };
    let listeners = vec![
        ("pointermove", listener(|_| Some(PointerMsgAction::Move))),
        (
            "pointerdown",
            listener(|e| button(e).map(PointerMsgAction::Press)),
        ),
        (
            "pointerup",
            listener(|e| button(e).map(PointerMsgAction::Release)),
        ),
        ("pointerleave", listener(|_| Some(PointerMsgAction::Leave))),
        // The browser took the pointer (a touch turned into a scroll, say).
        ("pointercancel", listener(|_| Some(PointerMsgAction::Leave))),
        // Capture also ends after every release; only a loss mid-press ends the press.
        (
            "lostpointercapture",
            listener(|e| (e.buttons() != 0).then_some(PointerMsgAction::Leave)),
        ),
    ];
    for (kind, listener) in &listeners {
        canvas.add_event_listener(kind, listener);
    }
    let subscription = Subscription {
        canvas,
        callback: callback.unchecked_into(),
        listeners,
    };
    SUBSCRIPTIONS.with(|s| s.borrow_mut().insert(id, subscription));
    Ok(())
}

fn button(e: &PointerEvent) -> Option<PointerButton> {
    match e.button() {
        0 => Some(PointerButton::Primary),
        1 => Some(PointerButton::Middle),
        2 => Some(PointerButton::Secondary),
        _ => None,
    }
}

fn unsubscribe(id: ViewId) {
    if let Some(picks) = PICKS.get() {
        picks.claim_only_bells(id, false);
    }
    let Some(gone) = SUBSCRIPTIONS.with(|s| s.borrow_mut().remove(&id)) else {
        return;
    };
    for (kind, listener) in &gone.listeners {
        gone.canvas.remove_event_listener(kind, listener);
    }
    if let Some(picks) = PICKS.get() {
        let msg = PointerMsg {
            action: PointerMsgAction::Leave,
            screen: Vec2::ZERO,
            size: size(&gone.canvas),
            modifiers: Modifiers::default(),
        };
        picks.pointer(id, msg);
    }
}

/// Calls each view's callback with the pick events of the frame.
fn deliver(picks: Res<PickHub>) {
    let serializer = serde_wasm_bindgen::Serializer::json_compatible();
    for (id, pick) in picks.take_picks() {
        // Cloned out so the callback may call `onPick` itself.
        let Some(callback) = SUBSCRIPTIONS.with(|s| {
            let s = s.borrow();
            s.get(&id)
                .map(|s| JsValue::clone(&s.callback).unchecked_into::<PickCallback>())
        }) else {
            continue;
        };
        if let Ok(event) = pick.serialize(&serializer)
            && let Err(error) = callback.call(&JsValue::NULL, &event)
        {
            console_error("onPick callback threw:", &error);
        }
    }
}

#[derive(Serialize)]
struct RayOutput {
    origin: [f32; 3],
    dir: [f32; 3],
}

#[derive(Deserialize)]
struct PlaneInput {
    normal: [f32; 3],
    constant: f32,
}

/// The view's camera ray through `(x, y)`, in CSS pixels from its canvas's top left.
fn view_ray(id: ViewId, x: f32, y: f32) -> Result<Ray3d, JsError> {
    let size = size(&view_canvas(id)?);
    CAMERAS
        .with(|c| {
            c.borrow()
                .get(&id)
                .and_then(|camera| ray_at(camera, size, Vec2::new(x, y)))
        })
        .ok_or_else(|| js_err(format!("view {id} has no camera, or no size")))
}

#[wasm_bindgen(js_name = rayAtScreen)]
pub fn ray_at_screen(id: ViewId, x: f32, y: f32) -> Result<JsValue, JsError> {
    let ray = view_ray(id, x, y)?;
    let ray = RayOutput {
        origin: ray.origin.into(),
        dir: ray.direction.as_vec3().into(),
    };
    serde_wasm_bindgen::to_value(&ray).map_err(js_err)
}

/// Where the ray through `(x, y)` meets `plane` (`normal · p + constant = 0`,
/// as three.js's `Plane`), or `null` when it doesn't, ahead of the camera.
#[wasm_bindgen(js_name = intersectPlane)]
pub fn intersect_plane(id: ViewId, x: f32, y: f32, plane: JsValue) -> Result<JsValue, JsError> {
    let plane: PlaneInput = serde_wasm_bindgen::from_value(plane).map_err(js_err)?;
    let normal = Vec3::from(plane.normal);
    if !normal.is_finite() || normal.length_squared() == 0.0 {
        return Err(js_err("the plane's normal must be non-zero"));
    }
    let point = plane_hit(view_ray(id, x, y)?, normal, plane.constant).map(<[f32; 3]>::from);
    point
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(js_err)
}
