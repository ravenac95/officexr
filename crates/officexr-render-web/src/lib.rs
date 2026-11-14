//! `officexr-render` in the browser, one view per canvas, fed from JS:
//! `init` once, then `createView` per canvas and `setWorld`, `setCamera`
//! and `setPlayers` on each.
//!
//! A view on the canvas given to `init` draws straight into Bevy's primary
//! window. With WebGPU every other view is a Bevy window on its own canvas.
//! WebGL2 can't do that: wgpu's GL backend binds the device to the primary
//! canvas's context, so a second window draws into the first canvas. There
//! each other view renders to an image, read back each frame and painted
//! into its canvas; without a primary canvas, Bevy's window sits on a
//! hidden 1 px host canvas.
#![cfg(target_arch = "wasm32")]

#[cfg(not(any(feature = "webgpu", feature = "webgl2")))]
compile_error!("enable exactly one of the `webgpu` and `webgl2` features");
#[cfg(all(feature = "webgpu", feature = "webgl2"))]
compile_error!("the `webgpu` and `webgl2` features are mutually exclusive");

use std::sync::OnceLock;

use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy::window::ExitCondition;
use officexr_render::views::{ViewHub, ViewId, ViewKind, ViewMsg, ViewStatus, ViewTarget};
use officexr_render::{
    AssetBase, OfficeXrRenderPlugin, PlayerRender, Players, SceneCamera, WorldDiff, mugshot_player,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

static HUB: OnceLock<ViewHub> = OnceLock::new();
/// Marks the canvas Bevy's primary window draws into.
const PRIMARY_ATTR: &str = "data-officexr-primary";

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = HTMLCanvasElement)]
    type Canvas;
    #[wasm_bindgen(method, js_name = setAttribute)]
    fn set_attribute(this: &Canvas, name: &str, value: &str);
    #[wasm_bindgen(method, js_name = hasAttribute)]
    fn has_attribute(this: &Canvas, name: &str) -> bool;
    #[wasm_bindgen(js_namespace = document, js_name = querySelector)]
    fn query_selector(selector: &str) -> JsValue;
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CameraInput {
    position: [f32; 3],
    look_at: [f32; 3],
    fov_deg: f32,
    near: f32,
    far: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayersInput {
    players: Vec<PlayerInput>,
    char_radius: f32,
    player_speed: f32,
    run_speed: f32,
}

#[derive(Deserialize)]
struct PlayerInput {
    id: String,
    character: String,
    pos: [f32; 3],
    yaw: f32,
    vel: [f32; 3],
    name: String,
    airborne: bool,
}

#[derive(Serialize)]
struct Readiness {
    ready: bool,
    pending: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FrameStats {
    draw_calls: usize,
    triangles: usize,
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

fn hub() -> Result<&'static ViewHub, JsError> {
    HUB.get().ok_or_else(|| js_err("call init first"))
}

fn no_view(id: ViewId) -> JsError {
    js_err(format!("no view {id}"))
}

fn send(id: ViewId, msg: ViewMsg) -> Result<(), JsError> {
    hub()?
        .send(id, msg)
        .then_some(())
        .ok_or_else(|| no_view(id))
}

fn status(id: ViewId) -> Result<ViewStatus, JsError> {
    hub()?.status(id).ok_or_else(|| no_view(id))
}

/// A canvas element, or the canvas a selector matches.
fn resolve_canvas(canvas: JsValue) -> Result<Canvas, JsError> {
    let canvas = match canvas.as_string() {
        Some(selector) => query_selector(&selector),
        None => canvas,
    };
    canvas
        .dyn_into()
        .map_err(|_| js_err("expected a canvas element or a selector matching one"))
}

/// Boots Bevy once; `origin` serves `/models/...`. A view later created on
/// `primary_canvas` draws into Bevy's primary window directly.
#[wasm_bindgen]
pub fn init(origin: String, primary_canvas: JsValue) -> Result<(), JsError> {
    if HUB.get().is_some() {
        return Ok(());
    }
    let primary_window = if primary_canvas.is_undefined() || primary_canvas.is_null() {
        host_window()
    } else {
        resolve_canvas(primary_canvas)?.set_attribute(PRIMARY_ATTR, "");
        Some(Window {
            canvas: Some(format!("canvas[{PRIMARY_ATTR}]")),
            fit_canvas_to_parent: true,
            ..default()
        })
    };
    let mut app = App::new();
    app.add_plugins((
        AssetBase::Http(origin),
        DefaultPlugins
            .set(WindowPlugin {
                primary_window,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            // The dev server answers missing `.meta` files with index.html.
            .set(AssetPlugin {
                meta_check: AssetMetaCheck::Never,
                ..default()
            }),
        OfficeXrRenderPlugin,
    ));
    #[cfg(feature = "webgl2")]
    image_views::add(&mut app);
    let _ = HUB.set(app.world().resource::<ViewHub>().clone());
    app.run();
    Ok(())
}

#[cfg(feature = "webgpu")]
fn host_window() -> Option<Window> {
    None
}

/// WebGL2 makes its device from the primary window's canvas.
#[cfg(feature = "webgl2")]
fn host_window() -> Option<Window> {
    Some(Window {
        canvas: Some(image_views::host_canvas()),
        resolution: (1, 1).into(),
        ..default()
    })
}

/// A view on `canvas` (an element or a selector), of kind `world` or `preview`.
#[wasm_bindgen(js_name = createView)]
pub fn create_view(canvas: JsValue, kind: &str) -> Result<ViewId, JsError> {
    let kind = match kind {
        "world" => ViewKind::World,
        "preview" => ViewKind::Preview,
        _ => return Err(js_err(format!("unknown view kind {kind:?}"))),
    };
    let canvas = resolve_canvas(canvas)?;
    Ok(hub()?.create(kind, |id| {
        if canvas.has_attribute(PRIMARY_ATTR) {
            return ViewTarget::PrimaryWindow;
        }
        canvas.set_attribute("data-officexr-view", &id.to_string());
        target(id, canvas)
    }))
}

#[cfg(feature = "webgpu")]
fn target(id: ViewId, _: Canvas) -> ViewTarget {
    ViewTarget::Window(format!("canvas[data-officexr-view=\"{id}\"]"))
}

#[cfg(feature = "webgl2")]
fn target(id: ViewId, canvas: Canvas) -> ViewTarget {
    ViewTarget::Image(image_views::register(id, canvas))
}

/// Despawns the view's window, camera and scene; Bevy's primary window stays, idle.
#[wasm_bindgen(js_name = destroyView)]
pub fn destroy_view(id: ViewId) -> Result<(), JsError> {
    send(id, ViewMsg::Destroy)?;
    #[cfg(feature = "webgl2")]
    image_views::unregister(id);
    Ok(())
}

/// Applies a world diff (`WorldDiff` JSON). With `mugshot`, that character
/// is settled on the diff's objects and drawn paused instead of the
/// `setPlayers` feed.
#[wasm_bindgen(js_name = setWorld)]
pub fn set_world(id: ViewId, diff_json: &str, mugshot: Option<String>) -> Result<(), JsError> {
    let diff: WorldDiff = serde_json::from_str(diff_json).map_err(js_err)?;
    send(id, ViewMsg::World(Box::new(diff)))?;
    if let Some(character) = mugshot {
        send(id, ViewMsg::Players(mugshot_player(diff_json, character)))?;
    }
    Ok(())
}

#[wasm_bindgen(js_name = setCamera)]
pub fn set_camera(id: ViewId, camera: JsValue) -> Result<(), JsError> {
    let camera: CameraInput = serde_wasm_bindgen::from_value(camera).map_err(js_err)?;
    send(
        id,
        ViewMsg::Camera(SceneCamera {
            position: camera.position,
            look_at: camera.look_at,
            fov_deg: camera.fov_deg,
            near: camera.near,
            far: camera.far,
        }),
    )
}

#[wasm_bindgen(js_name = setPlayers)]
pub fn set_players(id: ViewId, players: JsValue) -> Result<(), JsError> {
    let input: PlayersInput = serde_wasm_bindgen::from_value(players).map_err(js_err)?;
    let players = Players {
        players: input
            .players
            .into_iter()
            .map(|p| PlayerRender {
                id: p.id,
                character: p.character,
                pos: p.pos.into(),
                yaw: p.yaw,
                vel: p.vel.into(),
                name: p.name,
                airborne: p.airborne,
            })
            .collect(),
        char_radius: input.char_radius,
        player_speed: input.player_speed,
        run_speed: input.run_speed,
        ..default()
    };
    send(id, ViewMsg::Players(players))
}

/// `ready` once every model of the view has loaded and settled; `pending`
/// counts the models and avatars still loading.
#[wasm_bindgen(js_name = isReady)]
pub fn is_ready(id: ViewId) -> Result<JsValue, JsError> {
    let status = status(id)?;
    serde_wasm_bindgen::to_value(&Readiness {
        ready: status.ready,
        pending: status.pending,
    })
    .map_err(js_err)
}

#[wasm_bindgen(js_name = frameStats)]
pub fn frame_stats(id: ViewId) -> Result<JsValue, JsError> {
    let status = status(id)?;
    serde_wasm_bindgen::to_value(&FrameStats {
        draw_calls: status.draw_calls,
        triangles: status.triangles,
    })
    .map_err(js_err)
}

#[cfg(feature = "webgl2")]
mod image_views {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use bevy::prelude::*;
    use bevy::render::gpu_readback::ReadbackComplete;
    use bevy::render::renderer::RenderDevice;
    use officexr_render::views::{View, ViewId, ViewMsg};
    use wasm_bindgen::Clamped;
    use wasm_bindgen::prelude::*;

    use super::{Canvas, HUB};

    const HOST_ID: &str = "officexr-render-host";

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(method, getter, js_name = clientWidth)]
        fn client_width(this: &Canvas) -> f64;
        #[wasm_bindgen(method, getter, js_name = clientHeight)]
        fn client_height(this: &Canvas) -> f64;
        #[wasm_bindgen(method, getter)]
        fn width(this: &Canvas) -> u32;
        #[wasm_bindgen(method, setter)]
        fn set_width(this: &Canvas, width: u32);
        #[wasm_bindgen(method, getter)]
        fn height(this: &Canvas) -> u32;
        #[wasm_bindgen(method, setter)]
        fn set_height(this: &Canvas, height: u32);
        #[wasm_bindgen(method, js_name = getContext)]
        fn get_context(this: &Canvas, kind: &str) -> Context2d;
        #[wasm_bindgen(js_namespace = document, js_name = createElement)]
        fn create_element(tag: &str) -> Canvas;
        #[wasm_bindgen(js_namespace = ["document", "body"], js_name = append)]
        fn append_to_body(node: &Canvas);

        type Global;
        #[wasm_bindgen(thread_local_v2, js_name = globalThis)]
        static GLOBAL: Global;
        #[wasm_bindgen(method, getter, js_name = devicePixelRatio)]
        fn device_pixel_ratio(this: &Global) -> f64;

        type ResizeObserver;
        #[wasm_bindgen(constructor)]
        fn new(callback: &Closure<dyn FnMut()>) -> ResizeObserver;
        #[wasm_bindgen(method)]
        fn observe(this: &ResizeObserver, target: &Canvas);
        #[wasm_bindgen(method)]
        fn disconnect(this: &ResizeObserver);

        type Context2d;
        #[wasm_bindgen(method, js_name = putImageData)]
        #[allow(clippy::too_many_arguments)]
        fn put_image_data(
            this: &Context2d,
            data: &ImageData,
            x: f64,
            y: f64,
            dirty_x: f64,
            dirty_y: f64,
            dirty_width: f64,
            dirty_height: f64,
        );

        type ImageData;
        #[wasm_bindgen(constructor, catch)]
        fn new(data: Clamped<&[u8]>, width: u32, height: u32) -> Result<ImageData, JsValue>;
    }

    struct ImageView {
        canvas: Canvas,
        /// The size last asked of the view.
        size: UVec2,
        observer: ResizeObserver,
        _on_resize: Closure<dyn FnMut()>,
    }

    thread_local! {
        static VIEWS: RefCell<HashMap<ViewId, ImageView>> = RefCell::default();
    }

    pub(super) fn host_canvas() -> String {
        let host = create_element("canvas");
        host.set_attribute("id", HOST_ID);
        host.set_attribute(
            "style",
            "position:fixed;left:0;top:0;width:1px;height:1px;opacity:0;pointer-events:none",
        );
        append_to_body(&host);
        format!("#{HOST_ID}")
    }

    fn css_size(canvas: &Canvas) -> UVec2 {
        let ratio = GLOBAL.with(Global::device_pixel_ratio);
        UVec2::new(
            (canvas.client_width() * ratio).round() as u32,
            (canvas.client_height() * ratio).round() as u32,
        )
    }

    /// Sizes the view's image to its canvas's CSS size whenever that
    /// changes; a hidden canvas pauses the view.
    // ponytail: a devicePixelRatio change alone (browser zoom) isn't seen until the canvas resizes.
    pub(super) fn register(id: ViewId, canvas: Canvas) -> UVec2 {
        let size = css_size(&canvas);
        let on_resize = Closure::<dyn FnMut()>::new(move || {
            VIEWS.with(|views| {
                let mut views = views.borrow_mut();
                let Some(view) = views.get_mut(&id) else {
                    return;
                };
                let size = css_size(&view.canvas);
                if size != view.size
                    && let Some(hub) = HUB.get()
                {
                    view.size = size;
                    hub.send(id, ViewMsg::Resize(size));
                }
            });
        });
        let observer = ResizeObserver::new(&on_resize);
        observer.observe(&canvas);
        let view = ImageView {
            canvas,
            size,
            observer,
            _on_resize: on_resize,
        };
        VIEWS.with(|views| views.borrow_mut().insert(id, view));
        size
    }

    pub(super) fn unregister(id: ViewId) {
        if let Some(view) = VIEWS.with(|views| views.borrow_mut().remove(&id)) {
            view.observer.disconnect();
        }
    }

    pub(super) fn add(app: &mut App) {
        app.add_observer(paint);
    }

    /// Paints a view's read-back image into its canvas.
    fn paint(frame: On<ReadbackComplete>, views: Query<&View>) {
        let Ok(view) = views.get(frame.entity) else {
            return;
        };
        VIEWS.with(|views| {
            let views = views.borrow();
            let Some(ImageView { canvas, size, .. }) = views.get(&view.id) else {
                return;
            };
            let stride = RenderDevice::align_copy_bytes_per_row(size.x as usize * 4);
            // Zero-sized, or read back before the last resize.
            if size.x == 0 || size.y == 0 || frame.data.len() != stride * size.y as usize {
                return;
            }
            if canvas.width() != size.x || canvas.height() != size.y {
                canvas.set_width(size.x);
                canvas.set_height(size.y);
            }
            let context = canvas.get_context("2d");
            // A view into wasm memory, not a copy, so nothing may allocate before
            // `putImageData` copies it out. It is as wide as the padded rows; the
            // dirty rect leaves the padding out.
            let Ok(image) = ImageData::new(Clamped(&frame.data), stride as u32 / 4, size.y) else {
                return;
            };
            let (w, h) = (size.x as f64, size.y as f64);
            context.put_image_data(&image, 0.0, 0.0, 0.0, 0.0, w, h);
        });
    }
}
