//! Page-wide input that winit's canvas listeners miss: keys pressed while
//! focus is elsewhere on the page (winit only hears a focused canvas, and the
//! WebGL2 host canvas never takes focus), and the browser ending pointer lock
//! on its own.

use std::cell::RefCell;

use bevy::input::keyboard::{Key, KeyboardFocusLost, KeyboardInput, NativeKey};
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use officexr_controls::PointerLockLost;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    type EventTarget;
    #[wasm_bindgen(thread_local_v2, js_name = document)]
    static DOCUMENT: EventTarget;
    #[wasm_bindgen(thread_local_v2, js_name = globalThis)]
    static GLOBAL: EventTarget;
    #[wasm_bindgen(method, js_name = addEventListener)]
    fn add_event_listener(this: &EventTarget, kind: &str, listener: &JsValue, capture: bool);
    #[wasm_bindgen(method, getter, js_name = pointerLockElement)]
    fn pointer_lock_element(this: &EventTarget) -> JsValue;

    type KeyEvent;
    #[wasm_bindgen(method, getter)]
    fn code(this: &KeyEvent) -> String;
    #[wasm_bindgen(method, getter)]
    fn repeat(this: &KeyEvent) -> bool;
}

enum PageEvent {
    Key(KeyCode, ButtonState, bool),
    Blur,
    LockLost,
}

thread_local! {
    static EVENTS: RefCell<Vec<PageEvent>> = RefCell::default();
}

fn push(event: PageEvent) {
    EVENTS.with(|events| events.borrow_mut().push(event));
}

pub(super) fn add(app: &mut App) {
    listen();
    app.add_systems(PreUpdate, forward.before(InputSystems));
}

/// The listeners live as long as the page, like the app.
fn listen() {
    for (kind, state) in [
        ("keydown", ButtonState::Pressed),
        ("keyup", ButtonState::Released),
    ] {
        let on_key = Closure::<dyn FnMut(KeyEvent)>::new(move |event: KeyEvent| {
            if let Some(code) = key_code(&event.code()) {
                push(PageEvent::Key(code, state, event.repeat()));
            }
        });
        // Capture, so a handler that stops propagation can't hide a keyup.
        DOCUMENT.with(|d| d.add_event_listener(kind, on_key.as_ref(), true));
        on_key.forget();
    }
    let on_lock_change = Closure::<dyn FnMut()>::new(|| {
        if DOCUMENT.with(EventTarget::pointer_lock_element).is_null() {
            push(PageEvent::LockLost);
        }
    });
    let on_lock_error = Closure::<dyn FnMut()>::new(|| push(PageEvent::LockLost));
    DOCUMENT.with(|d| {
        d.add_event_listener("pointerlockchange", on_lock_change.as_ref(), false);
        d.add_event_listener("pointerlockerror", on_lock_error.as_ref(), false);
    });
    on_lock_change.forget();
    on_lock_error.forget();
    // Keys released while the page is in the background never send a keyup.
    let on_blur = Closure::<dyn FnMut()>::new(|| push(PageEvent::Blur));
    GLOBAL.with(|g| g.add_event_listener("blur", on_blur.as_ref(), false));
    on_blur.forget();
}

/// Hands the page's events to Bevy. A key the focused canvas also reports
/// arrives twice, which `ButtonInput` takes as one press.
fn forward(
    mut keys: MessageWriter<KeyboardInput>,
    mut focus_lost: MessageWriter<KeyboardFocusLost>,
    mut lock_lost: MessageWriter<PointerLockLost>,
    primary: Option<Single<Entity, With<PrimaryWindow>>>,
) {
    let window = primary.map_or(Entity::PLACEHOLDER, |p| *p);
    for event in EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut())) {
        match event {
            PageEvent::Key(key_code, state, repeat) => {
                keys.write(KeyboardInput {
                    key_code,
                    logical_key: Key::Unidentified(NativeKey::Unidentified),
                    state,
                    text: None,
                    repeat,
                    window,
                });
            }
            PageEvent::Blur => {
                focus_lost.write(KeyboardFocusLost);
            }
            PageEvent::LockLost => {
                lock_lost.write(PointerLockLost);
            }
        }
    }
}

/// A DOM `KeyboardEvent.code` the controls read.
// ponytail: only the controls' keys; anything else still reaches Bevy only through a focused canvas.
fn key_code(code: &str) -> Option<KeyCode> {
    Some(match code {
        "KeyW" => KeyCode::KeyW,
        "KeyA" => KeyCode::KeyA,
        "KeyS" => KeyCode::KeyS,
        "KeyD" => KeyCode::KeyD,
        "KeyQ" => KeyCode::KeyQ,
        "KeyE" => KeyCode::KeyE,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "Space" => KeyCode::Space,
        "Escape" => KeyCode::Escape,
        "ShiftLeft" => KeyCode::ShiftLeft,
        "ShiftRight" => KeyCode::ShiftRight,
        "ControlLeft" => KeyCode::ControlLeft,
        "ControlRight" => KeyCode::ControlRight,
        "AltLeft" => KeyCode::AltLeft,
        "AltRight" => KeyCode::AltRight,
        "MetaLeft" | "OSLeft" => KeyCode::SuperLeft,
        "MetaRight" | "OSRight" => KeyCode::SuperRight,
        _ => return None,
    })
}
