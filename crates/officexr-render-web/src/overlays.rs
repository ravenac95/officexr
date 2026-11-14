//! `setOverlays`: the editors' outlines, ghosts, grid, gizmo, markers,
//! zones and labels over a view's world.

use officexr_render::overlays::Overlay;
use officexr_render::views::{ViewId, ViewMsg};
use wasm_bindgen::prelude::*;

use super::{js_err, send, to_json};

/// Replaces the view's overlays; only those whose `tag` is new or whose
/// content changed are redrawn.
#[wasm_bindgen(js_name = setOverlays)]
pub fn set_overlays(id: ViewId, overlays: JsValue) -> Result<(), JsError> {
    let json = to_json(&overlays).ok_or_else(|| js_err("overlays must be an array"))?;
    let overlays: Vec<Overlay> = serde_json::from_str(&json).map_err(js_err)?;
    send(id, ViewMsg::Overlays(overlays))
}
