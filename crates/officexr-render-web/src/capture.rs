//! `captureView`: a view's next frame as a PNG `Blob`, through Bevy's screenshot API.

use std::cell::RefCell;
use std::collections::HashMap;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use officexr_render::straight_rgba;
use officexr_render::views::{View, ViewId};
use wasm_bindgen::Clamped;
use wasm_bindgen::prelude::*;

use super::{js_err, status};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = Promise)]
    pub type JsPromise;
    #[wasm_bindgen(constructor, js_class = Promise)]
    fn new(executor: &mut dyn FnMut(JsValue, JsValue)) -> JsPromise;

    type Settle;
    #[wasm_bindgen(method, js_name = call)]
    fn call(this: &Settle, this_arg: &JsValue, value: &JsValue) -> JsValue;

    type OffscreenCanvas;
    #[wasm_bindgen(constructor)]
    fn new(width: u32, height: u32) -> OffscreenCanvas;
    #[wasm_bindgen(method, js_name = getContext)]
    fn get_context(this: &OffscreenCanvas, kind: &str) -> Context2d;
    /// A `Promise<Blob>`.
    #[wasm_bindgen(method, js_name = convertToBlob)]
    fn convert_to_blob(this: &OffscreenCanvas) -> JsValue;

    type Context2d;
    #[wasm_bindgen(method, js_name = putImageData)]
    fn put_image_data(this: &Context2d, data: &ImageData, x: f64, y: f64);

    type ImageData;
    #[wasm_bindgen(constructor, catch)]
    fn new(data: Clamped<&[u8]>, width: u32, height: u32) -> Result<ImageData, JsValue>;
}

struct Pending {
    view: ViewId,
    /// Its screenshot is on its way.
    taken: bool,
    resolve: Settle,
    reject: Settle,
}

thread_local! {
    static NEXT: RefCell<u32> = const { RefCell::new(0) };
    /// Requests by number, until the app takes their screenshot.
    static REQUESTS: RefCell<HashMap<u32, Pending>> = RefCell::default();
}

pub(super) fn add(app: &mut App) {
    app.add_systems(Update, take_screenshots);
}

/// Resolves to the view's next frame, as a PNG with its alpha.
#[wasm_bindgen(js_name = captureView)]
pub fn capture_view(id: ViewId) -> Result<JsPromise, JsError> {
    status(id)?;
    let request = NEXT.with(|n| {
        let mut n = n.borrow_mut();
        *n += 1;
        *n
    });
    Ok(JsPromise::new(&mut |resolve, reject| {
        let pending = Pending {
            view: id,
            taken: false,
            resolve: resolve.unchecked_into(),
            reject: reject.unchecked_into(),
        };
        REQUESTS.with(|r| r.borrow_mut().insert(request, pending));
    }))
}

fn settle(request: u32, outcome: Result<JsValue, JsValue>) {
    let Some(pending) = REQUESTS.with(|r| r.borrow_mut().remove(&request)) else {
        return;
    };
    match outcome {
        Ok(value) => pending.resolve.call(&JsValue::NULL, &value),
        Err(error) => pending.reject.call(&JsValue::NULL, &error),
    };
}

/// Rejects the view's captures still pending: it is going, and its
/// screenshots with it.
pub(super) fn forget(id: ViewId) {
    let gone: Vec<u32> = REQUESTS.with(|r| {
        r.borrow()
            .iter()
            .filter(|(_, p)| p.view == id)
            .map(|(n, _)| *n)
            .collect()
    });
    for request in gone {
        settle(
            request,
            Err(js_err(format!("view {id} was destroyed")).into()),
        );
    }
}

/// Spawns a screenshot of each requested view's target, once its view is drawn.
fn take_screenshots(mut commands: Commands, views: Query<&View>) {
    let requests: Vec<(u32, ViewId)> = REQUESTS.with(|r| {
        r.borrow()
            .iter()
            .filter(|(_, p)| !p.taken)
            .map(|(n, p)| (*n, p.view))
            .collect()
    });
    for (request, id) in requests {
        // A view made this frame has no entity yet.
        let Some(view) = views.iter().find(|v| v.id == id) else {
            continue;
        };
        REQUESTS.with(|r| {
            if let Some(p) = r.borrow_mut().get_mut(&request) {
                p.taken = true;
            }
        });
        let screenshot = match view.target() {
            RenderTarget::Image(image) => Screenshot::image(image.handle.clone()),
            target => Screenshot(target.clone()),
        };
        commands
            .spawn(screenshot)
            .observe(move |captured: On<ScreenshotCaptured>| {
                settle(request, png_blob(&captured.image));
            });
    }
}

/// A `Promise<Blob>` of the image as a PNG.
fn png_blob(image: &Image) -> Result<JsValue, JsValue> {
    let rgba = straight_rgba(image).map_err(|e| JsValue::from(js_err(e)))?;
    let (width, height) = (image.width(), image.height());
    let data = ImageData::new(Clamped(&rgba), width, height)?;
    let canvas = OffscreenCanvas::new(width, height);
    canvas.get_context("2d").put_image_data(&data, 0.0, 0.0);
    Ok(canvas.convert_to_blob())
}
