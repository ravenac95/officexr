//! One module per `packages/world/src/physics/*.ts` rule file, plus the
//! Rapier world the TS code gets from rapier.js.

pub mod air_control;
pub mod blocking;
pub mod character_movement;
pub mod collider_sync;
pub mod groups;
pub mod kcc_step_up_probe;
pub mod rapier_world;
pub mod rules;
pub mod step_up;
