//! The local player's headless movement simulation, ported from the TS
//! `TsSimCore` (`packages/world/src/sim/`) onto the same Rapier release.

mod geometry;
pub mod physics;
mod sim_core;
pub mod trace;
mod world_objects;

pub use physics::character_movement::{AnimState, PeerBump};
pub use physics::rules::{ColliderDescriptor, ColliderShapeSpec};
pub use sim_core::{
    SimBakedColliders, SimCore, SimInput, SimKind, SimKinds, SimPeer, SimSettings, SimStepResult,
};
pub use world_objects::{ObjectInstance, Vec3, WorldObjects};
