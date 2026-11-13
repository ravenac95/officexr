//! Golden records of a `SimCore` run (`packages/world/src/sim/trace.ts`).

use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::physics::character_movement::XZ;
use crate::physics::collider_sync::InvalidTrimesh;
use crate::proximity::{MeetingArea, PairState, ProximityEvent, ProximityInput, TrackerOutput};
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
pub fn replay(
    sim: &mut SimCore,
    map: &SimMap,
    inputs: &[SimTraceFrame],
) -> Result<Vec<SimStepResult>, InvalidTrimesh> {
    sim.set_static_colliders(&map.world_objects, &map.kinds, &map.baked_colliders)?;
    Ok(inputs
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
        .collect())
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProximityTraceFrame {
    #[serde(deserialize_with = "players_in_key_order")]
    pub players: Vec<SimPeer>,
    pub input: ProximityInput,
}

/// `TrackerOutput` as recorded: maps as key-sorted entry lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProximityFrameOutput {
    pub events: Vec<ProximityEvent>,
    pub pair_states: Vec<(String, PairState)>,
    pub player_states: Vec<(String, PairState)>,
    pub areas: Vec<MeetingArea>,
    pub conversation_focus: Option<XZ>,
}

impl From<TrackerOutput> for ProximityFrameOutput {
    fn from(out: TrackerOutput) -> Self {
        Self {
            events: out.events,
            pair_states: out.pair_states.into_iter().collect(),
            player_states: out.player_states.into_iter().collect(),
            areas: out.areas,
            conversation_focus: out.conversation_focus,
        }
    }
}

/// `outputs[i]` is the proximity tick result of `inputs[i]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProximityTrace {
    pub inputs: Vec<ProximityTraceFrame>,
    pub outputs: Vec<ProximityFrameOutput>,
}

/// Plays `inputs` on `sim`, returning what each proximity tick returns.
pub fn replay_proximity(
    sim: &mut SimCore,
    inputs: &[ProximityTraceFrame],
) -> Vec<ProximityFrameOutput> {
    inputs
        .iter()
        .map(|f| sim.tick_proximity(&f.players, &f.input).into())
        .collect()
}

/// A JSON players record as a list in its key order, which the tracker's
/// event order depends on.
fn players_in_key_order<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<SimPeer>, D::Error> {
    struct InOrder;
    impl<'de> Visitor<'de> for InOrder {
        type Value = Vec<SimPeer>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a map of players by id")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut players = Vec::new();
            while let Some((_, p)) = map.next_entry::<String, SimPeer>()? {
                players.push(p);
            }
            Ok(players)
        }
    }
    d.deserialize_map(InOrder)
}
