//! `setProximity`: the conversation glow under a world view's players.

use officexr_render::proximity::Proximity;
use officexr_render::views::{ViewId, ViewMsg};
use wasm_bindgen::prelude::*;

use super::{js_err, send, to_json};

/// Replaces the view's proximity output; the glow eases toward it.
#[wasm_bindgen(js_name = setProximity)]
pub fn set_proximity(id: ViewId, proximity: JsValue) -> Result<(), JsError> {
    let json = to_json(&proximity).ok_or_else(|| js_err("proximity must be an object"))?;
    let proximity: Proximity = serde_json::from_str(&json).map_err(js_err)?;
    send(id, ViewMsg::Proximity(proximity))
}
