//! The play view's world effects: `setOcclusion` and `occlusionStats` (the
//! objects that hide the player fade, the roof over them is cut away),
//! `setWater` (the pools' surfaces) and `setScreens` (desk monitors lit
//! while their chair is taken).

use std::cell::RefCell;
use std::collections::HashMap;

use bevy::prelude::*;
use officexr_render::occlusion::{OcclusionInput, OcclusionStats};
use officexr_render::screens::ScreenInput;
use officexr_render::views::{View, ViewId, ViewMsg};
use officexr_render::water::WaterVolume;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use super::{js_err, send, status, to_json};

thread_local! {
    /// Each view's occlusion on its last frame, for the synchronous `occlusionStats`.
    static STATS: RefCell<HashMap<ViewId, OcclusionStats>> = RefCell::default();
}

pub(super) fn add(app: &mut App) {
    app.add_systems(Last, mirror_stats);
}

pub(super) fn forget(id: ViewId) {
    STATS.with(|s| s.borrow_mut().remove(&id));
}

fn mirror_stats(views: Query<(&View, &OcclusionStats), Changed<OcclusionStats>>) {
    STATS.with(|s| {
        let mut s = s.borrow_mut();
        for (view, stats) in &views {
            s.insert(view.id, stats.clone());
        }
    });
}

fn parse<T: serde::de::DeserializeOwned>(value: &JsValue, what: &str) -> Result<T, JsError> {
    let json = to_json(value).ok_or_else(|| js_err(format!("{what} must be JSON")))?;
    serde_json::from_str(&json).map_err(js_err)
}

/// `{ targets, ignore?, anchor?, cutaway?, occupants? }`, or `null` to restore every occluder.
#[wasm_bindgen(js_name = setOcclusion)]
pub fn set_occlusion(id: ViewId, input: JsValue) -> Result<(), JsError> {
    let input: Option<OcclusionInput> = if input.is_null() || input.is_undefined() {
        None
    } else {
        Some(parse(&input, "occlusion")?)
    };
    send(id, ViewMsg::Occlusion(input))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatsOutput<'a> {
    fading: usize,
    cutaway: usize,
    silhouette_suppressed: &'a [String],
}

/// What occlusion did on the view's last frame.
#[wasm_bindgen(js_name = occlusionStats)]
pub fn occlusion_stats(id: ViewId) -> Result<JsValue, JsError> {
    status(id)?;
    STATS.with(|s| {
        let s = s.borrow();
        let stats = s.get(&id).cloned().unwrap_or_default();
        let output = StatsOutput {
            fading: stats.fading,
            cutaway: stats.cut,
            silhouette_suppressed: &stats.silhouette_suppressed,
        };
        output
            .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
            .map_err(js_err)
    })
}

/// Replaces the view's pools' water: `WaterVolume`s, as `motion/water.ts` makes them.
#[wasm_bindgen(js_name = setWater)]
pub fn set_water(id: ViewId, volumes: JsValue) -> Result<(), JsError> {
    let volumes: Vec<WaterVolume> = parse(&volumes, "water")?;
    send(id, ViewMsg::Water(volumes))
}

/// Replaces the view's lit screens: `{ id, glow, toward }` each; a screen left out is off.
#[wasm_bindgen(js_name = setScreens)]
pub fn set_screens(id: ViewId, screens: JsValue) -> Result<(), JsError> {
    let screens: Vec<ScreenInput> = parse(&screens, "screens")?;
    if screens
        .iter()
        .any(|s| !s.glow.is_finite() || !s.toward.iter().all(|v| v.is_finite()))
    {
        return Err(js_err("numbers must be finite"));
    }
    send(id, ViewMsg::Screens(screens))
}
