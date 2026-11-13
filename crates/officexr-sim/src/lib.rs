//! The local player's headless movement simulation, ported from the TS
//! `TsSimCore` (`packages/world/src/sim/`) onto the same Rapier release.

mod geometry;
pub mod physics;
mod proximity;
mod sim_core;
pub mod trace;
mod world_objects;

pub use physics::character_movement::{AnimState, PeerBump};
pub use physics::collider_sync::InvalidTrimesh;
pub use physics::rules::{ColliderDescriptor, ColliderShapeSpec};
pub use proximity::{
    EXTRAPOLATION_CAP_S, MeetingArea, PairState, PairTracker, ProximityEvent, ProximityEventKind,
    ProximityInput, ProximitySettings, TrackerOutput, VoiceZone, extrapolate_pos,
};
pub use sim_core::{
    SimBakedColliders, SimCore, SimInput, SimKind, SimKinds, SimPeer, SimSettings, SimStepResult,
};
pub use world_objects::{ObjectInstance, Vec3, WorldObjects};
