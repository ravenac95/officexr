//! `captureView`: a view's next frame as a PNG `Blob`, through Bevy's screenshot API.

use std::cell::RefCell;
use std::collections::HashMap;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
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

/// Spawns a screenshot of each requested view's target, once.
fn take_screenshots(mut commands: Commands, views: Query<&View>, mut taken: Local<Vec<u32>>) {
    let requests: Vec<(u32, ViewId)> = REQUESTS.with(|r| {
        r.borrow()
            .iter()
            .filter(|(n, _)| !taken.contains(n))
            .map(|(n, p)| (*n, p.view))
            .collect()
    });
    taken.retain(|n| REQUESTS.with(|r| r.borrow().contains_key(n)));
    for (request, id) in requests {
        taken.push(request);
        let Some(view) = views.iter().find(|v| v.id == id) else {
            settle(request, Err(js_err(format!("no view {id}")).into()));
            continue;
        };
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
    let (width, height) = (image.width(), image.height());
    let mut rgba = image
        .data
        .clone()
        .ok_or_else(|| JsValue::from(js_err("the screenshot has no pixels")))?;
    match image.texture_descriptor.format {
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => {}
        // Swap chains are often BGRA.
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => {
            for px in rgba.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
        }
        format => {
            return Err(js_err(format!("can't capture a {format:?} target")).into());
        }
    }
    let data = ImageData::new(Clamped(&rgba), width, height)?;
    let canvas = OffscreenCanvas::new(width, height);
    canvas.get_context("2d").put_image_data(&data, 0.0, 0.0);
    Ok(canvas.convert_to_blob())
}
