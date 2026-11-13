//! Golden records of a `SimCore` run (`packages/world/src/sim/trace.ts`).

use serde::{Deserialize, Serialize};

use crate::sim_core::{SimBakedColliders, SimCore, SimInput, SimKinds, SimPeer, SimStepResult};
use crate::world_objects::{Vec3, WorldObjects};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimMap {
    pub world_objects: WorldObjects,
    pub kinds: SimKinds,
    pub baked_colliders: Vec<SimBakedColliders>,
}

/// One frame: an optional teleport, then an optional peer sync, then the step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimTraceFrame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teleport: Option<Vec3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peers: Option<Vec<SimPeer>>,
    pub step: SimInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SimTraceKind {
    Movement,
}

/// `outputs[i]` is the step result of `inputs[i]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimTrace {
    pub kind: SimTraceKind,
    pub map: SimMap,
    pub inputs: Vec<SimTraceFrame>,
    pub outputs: Vec<SimStepResult>,
}

/// Plays `inputs` on `sim` over `map`, returning what each step returns.
pub fn replay(sim: &mut SimCore, map: &SimMap, inputs: &[SimTraceFrame]) -> Vec<SimStepResult> {
    sim.set_static_colliders(&map.world_objects, &map.kinds, &map.baked_colliders);
    inputs
        .iter()
        .map(|frame| {
            if let Some(pos) = frame.teleport {
                sim.teleport(pos);
            }
            if let Some(peers) = &frame.peers {
                sim.sync_peers(peers, frame.step.settings.char_radius);
            }
            sim.step(&frame.step)
        })
        .collect()
}
