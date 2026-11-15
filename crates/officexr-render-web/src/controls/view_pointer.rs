//! A WebGL2 image view's canvas never reaches Bevy's windows, so its drags
//! and wheel go to the controls from its own DOM events.

use std::cell::RefCell;
use std::collections::HashMap;

use bevy::input::mouse::MouseScrollUnit;
use bevy::prelude::*;
use officexr_controls::{ControlsSystems, ViewPointer, ViewPointerInput};
use officexr_render::views::ViewId;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::Canvas;

#[wasm_bindgen]
extern "C" {
    type DomEvent;
    #[wasm_bindgen(method, js_name = preventDefault)]
    fn prevent_default(this: &DomEvent);
    #[wasm_bindgen(method, getter)]
    fn button(this: &DomEvent) -> i16;
    #[wasm_bindgen(method, getter)]
    fn buttons(this: &DomEvent) -> u16;
    #[wasm_bindgen(method, getter, js_name = pointerId)]
    fn pointer_id(this: &DomEvent) -> i32;
    #[wasm_bindgen(method, getter, js_name = movementX)]
    fn movement_x(this: &DomEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = movementY)]
    fn movement_y(this: &DomEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = deltaY)]
    fn delta_y(this: &DomEvent) -> f64;
    #[wasm_bindgen(method, getter, js_name = deltaMode)]
    fn delta_mode(this: &DomEvent) -> u32;

    #[wasm_bindgen(method, js_name = addEventListener)]
    fn add_listener(this: &Canvas, kind: &str, listener: &Listener, options: &JsValue);
    #[wasm_bindgen(method, js_name = removeEventListener)]
    fn remove_listener(this: &Canvas, kind: &str, listener: &Listener);
    #[wasm_bindgen(method, js_name = setPointerCapture)]
    fn capture_pointer(this: &Canvas, id: i32);
}

type Listener = Closure<dyn FnMut(DomEvent)>;

struct Listening {
    canvas: Canvas,
    listeners: Vec<(&'static str, Listener)>,
}

thread_local! {
    static LISTENING: RefCell<HashMap<ViewId, Listening>> = RefCell::default();
    static FED: RefCell<Vec<ViewPointerInput>> = RefCell::default();
}

#[derive(Serialize)]
struct Options {
    /// So the wheel can stop the page scrolling.
    passive: bool,
}

pub(crate) fn add(app: &mut App) {
    app.add_systems(Update, forward.before(ControlsSystems));
}

fn forward(mut inputs: MessageWriter<ViewPointerInput>) {
    inputs.write_batch(FED.with(|f| std::mem::take(&mut *f.borrow_mut())));
}

fn button(e: &DomEvent) -> Option<MouseButton> {
    match e.button() {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Middle),
        2 => Some(MouseButton::Right),
        _ => None,
    }
}

/// Feeds the controls `id`'s pointer from `canvas` until [`forget`].
pub(crate) fn listen(id: ViewId, canvas: Canvas) {
    let feed =
        move |pointer| FED.with(|f| f.borrow_mut().push(ViewPointerInput { view: id, pointer }));
    let capture: Canvas = JsValue::clone(&canvas).unchecked_into();
    let listeners: Vec<(&'static str, Listener)> = vec![
        (
            "pointerdown",
            Listener::new(move |e: DomEvent| {
                if let Some(b) = button(&e) {
                    // Drags keep going past the canvas's edge.
                    capture.capture_pointer(e.pointer_id());
                    feed(ViewPointer::Press(b));
                }
            }),
        ),
        (
            "pointerup",
            Listener::new(move |e: DomEvent| {
                if let Some(b) = button(&e) {
                    feed(ViewPointer::Release(b));
                }
            }),
        ),
        (
            "pointermove",
            Listener::new(move |e: DomEvent| {
                feed(ViewPointer::Move(Vec2::new(
                    e.movement_x() as f32,
                    e.movement_y() as f32,
                )));
            }),
        ),
        (
            "pointercancel",
            Listener::new(move |_| feed(ViewPointer::Lost)),
        ),
        // Capture also ends after every release; only a loss mid-press ends the drag.
        (
            "lostpointercapture",
            Listener::new(move |e: DomEvent| {
                if e.buttons() != 0 {
                    feed(ViewPointer::Lost);
                }
            }),
        ),
        (
            "wheel",
            Listener::new(move |e: DomEvent| {
                e.prevent_default();
                let unit = if e.delta_mode() == 0 {
                    MouseScrollUnit::Pixel
                } else {
                    MouseScrollUnit::Line
                };
                // Bevy's wheel is positive scrolling up; the DOM's down.
                feed(ViewPointer::Wheel(-e.delta_y() as f32, unit));
            }),
        ),
        // A right-drag orbits instead of opening the menu.
        (
            "contextmenu",
            Listener::new(|e: DomEvent| e.prevent_default()),
        ),
    ];
    let options = serde_wasm_bindgen::to_value(&Options { passive: false }).unwrap_or_default();
    for (kind, listener) in &listeners {
        canvas.add_listener(kind, listener, &options);
    }
    LISTENING.with(|l| l.borrow_mut().insert(id, Listening { canvas, listeners }));
}

pub(crate) fn forget(id: ViewId) {
    if let Some(gone) = LISTENING.with(|l| l.borrow_mut().remove(&id)) {
        for (kind, listener) in &gone.listeners {
            gone.canvas.remove_listener(kind, listener);
        }
    }
}
