//! wasm-bindgen bindings for `officexr-sim`, wrapped by the TS `WasmSimCore`.
//! Arguments and results cross as structured JS values via serde-wasm-bindgen.

use officexr_sim::trace::SimMap;
use officexr_sim::{ProximityInput, SimCore, SimInput, SimPeer, Vec3};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;

fn from_js<T: DeserializeOwned>(value: JsValue) -> Result<T, JsError> {
    serde_wasm_bindgen::from_value(value).map_err(|e| JsError::new(&e.to_string()))
}

/// Maps become JS `Map`s and `None` becomes `null`, as in the TS outputs.
fn to_js<T: Serialize>(value: &T) -> JsValue {
    value
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))
        .expect("sim outputs serialize")
}

#[wasm_bindgen]
pub struct WasmSim(SimCore);

#[wasm_bindgen]
impl WasmSim {
    #[wasm_bindgen(constructor)]
    pub fn new(char_radius: f64) -> WasmSim {
        WasmSim(SimCore::new(char_radius))
    }

    /// Takes a `SimMap`: `{ worldObjects, kinds, bakedColliders }`.
    #[wasm_bindgen(js_name = setStaticColliders)]
    pub fn set_static_colliders(&mut self, map: JsValue) -> Result<(), JsError> {
        let map: SimMap = from_js(map)?;
        self.0
            .set_static_colliders(&map.world_objects, &map.kinds, &map.baked_colliders)
            .map_err(|e| JsError::new(&e.0))
    }

    #[wasm_bindgen(js_name = syncPeers)]
    pub fn sync_peers(&mut self, peers: JsValue, char_radius: f64) -> Result<(), JsError> {
        let peers: Vec<SimPeer> = from_js(peers)?;
        self.0.sync_peers(&peers, char_radius);
        Ok(())
    }

    pub fn step(&mut self, input: JsValue) -> Result<JsValue, JsError> {
        let input: SimInput = from_js(input)?;
        Ok(to_js(&self.0.step(&input)))
    }

    /// `players` is a `SimPeer` list in the TS `Object.keys` order, which event order follows.
    #[wasm_bindgen(js_name = tickProximity)]
    pub fn tick_proximity(&mut self, players: JsValue, input: JsValue) -> Result<JsValue, JsError> {
        let players: Vec<SimPeer> = from_js(players)?;
        let input: ProximityInput = from_js(input)?;
        Ok(to_js(&self.0.tick_proximity(&players, &input)))
    }

    pub fn position(&self) -> JsValue {
        to_js(&self.0.position())
    }

    pub fn teleport(&mut self, pos: JsValue) -> Result<(), JsError> {
        let pos: Vec3 = from_js(pos)?;
        self.0.teleport(pos);
        Ok(())
    }

    #[wasm_bindgen(js_name = setGravityEnabled)]
    pub fn set_gravity_enabled(&mut self, on: bool) {
        self.0.set_gravity_enabled(on);
    }
}
