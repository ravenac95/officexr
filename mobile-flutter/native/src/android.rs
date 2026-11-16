//! JNI entry points for `dev.officexr.bevyflutter.BevyBridge` (see the Flutter app's Kotlin).
//!
//! Kotlin hands over the `Surface` Flutter composites as a texture; Bevy renders into its
//! `ANativeWindow` on a thread of its own. Events go back to a Kotlin listener from a second
//! thread, so the render loop never blocks on the JVM.

use std::sync::Mutex;
use std::thread::JoinHandle;

use bevy::input::touch::TouchPhase;
use bevy::window::{RawHandleWrapper, WindowWrapper};
use crossbeam_channel::Sender;
use jni::objects::{JObject, JString, JValue};
use jni::sys::{jfloat, jint, jlong};
use jni::{JNIEnv, JavaVM};
use ndk::native_window::NativeWindow;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use crate::host::{self, HostInput, SurfaceTarget};
use crate::scene::{HostEvent, SceneCommand};

/// The `android-activity` glue Bevy links in expects this symbol. Flutter owns the
/// activity, so it never runs.
#[unsafe(no_mangle)]
fn android_main(_app: android_activity::AndroidApp) {}

struct Running {
    inputs: Sender<HostInput>,
    render: JoinHandle<()>,
    forward: JoinHandle<()>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

struct AndroidSurface(NativeWindow);

impl HasWindowHandle for AndroidSurface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for AndroidSurface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::android())
    }
}

fn send(input: HostInput) {
    if let Some(running) = RUNNING.lock().unwrap().as_ref() {
        let _ = running.inputs.send(input);
    }
}

fn stop() {
    let Some(running) = RUNNING.lock().unwrap().take() else {
        return;
    };
    let _ = running.inputs.send(HostInput::Stop);
    // The render thread drops the app (and its event sender), which ends the forwarder.
    let _ = running.render.join();
    let _ = running.forward.join();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_officexr_bevyflutter_BevyBridge_nativeStart(
    mut env: JNIEnv,
    _this: JObject,
    surface: JObject,
    width: jint,
    height: jint,
    scale: jfloat,
    listener: JObject,
) {
    stop();
    // SAFETY: `surface` is a live android.view.Surface for the duration of this call, and
    // `from_surface` takes its own reference to the native window.
    let window = unsafe { NativeWindow::from_surface(env.get_raw(), surface.as_raw()) };
    let Some(window) = window else {
        let _ = env.throw_new("java/lang/IllegalStateException", "Surface has no native window");
        return;
    };
    let handle = match RawHandleWrapper::new(&WindowWrapper::new(AndroidSurface(window))) {
        Ok(handle) => handle,
        Err(err) => {
            let _ = env.throw_new("java/lang/IllegalStateException", err.to_string());
            return;
        }
    };
    let (vm, listener) = match (env.get_java_vm(), env.new_global_ref(listener)) {
        (Ok(vm), Ok(listener)) => (vm, listener),
        _ => return,
    };

    let (input_tx, input_rx) = crossbeam_channel::unbounded();
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<HostEvent>();
    let target = SurfaceTarget {
        handle,
        width: width.max(1) as u32,
        height: height.max(1) as u32,
        scale,
    };
    let render_events = event_tx.clone();
    let render = std::thread::Builder::new()
        .name("bevy-render-host".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                host::run(target, input_rx, render_events.clone());
            }));
            if let Err(panic) = result {
                let message = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "Bevy panicked".into());
                let _ = render_events.send(HostEvent::Error { message });
            }
        })
        .expect("spawning the render thread");
    drop(event_tx);
    let forward = std::thread::Builder::new()
        .name("bevy-events".into())
        .spawn(move || forward_events(vm, listener, event_rx))
        .expect("spawning the event thread");

    *RUNNING.lock().unwrap() = Some(Running {
        inputs: input_tx,
        render,
        forward,
    });
}

fn forward_events(
    vm: JavaVM,
    listener: jni::objects::GlobalRef,
    events: crossbeam_channel::Receiver<HostEvent>,
) {
    let Ok(mut env) = vm.attach_current_thread() else {
        return;
    };
    for event in events {
        let Ok(json) = serde_json::to_string(&event) else {
            continue;
        };
        let Ok(text) = env.new_string(json) else {
            continue;
        };
        let _ = env.call_method(
            listener.as_obj(),
            "onEvent",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&text)],
        );
        let _ = env.delete_local_ref(text);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_officexr_bevyflutter_BevyBridge_nativeTouch(
    _env: JNIEnv,
    _this: JObject,
    id: jlong,
    phase: jint,
    x: jfloat,
    y: jfloat,
) {
    let phase = match phase {
        0 => TouchPhase::Started,
        1 => TouchPhase::Moved,
        2 => TouchPhase::Ended,
        _ => TouchPhase::Canceled,
    };
    send(HostInput::Touch {
        id: id as u64,
        phase,
        x,
        y,
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_officexr_bevyflutter_BevyBridge_nativeResize(
    _env: JNIEnv,
    _this: JObject,
    width: jint,
    height: jint,
) {
    send(HostInput::Resize {
        width: width.max(1) as u32,
        height: height.max(1) as u32,
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_officexr_bevyflutter_BevyBridge_nativeCommand(
    mut env: JNIEnv,
    _this: JObject,
    json: JString,
) {
    let Ok(json) = env.get_string(&json).map(String::from) else {
        return;
    };
    match serde_json::from_str::<SceneCommand>(&json) {
        Ok(command) => send(HostInput::Scene(command)),
        Err(err) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", err.to_string());
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_officexr_bevyflutter_BevyBridge_nativeStop(
    _env: JNIEnv,
    _this: JObject,
) {
    stop();
}
