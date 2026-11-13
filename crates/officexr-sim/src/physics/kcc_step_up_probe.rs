use rapier3d::prelude::*;

use super::collider_sync::ColliderSync;
use super::rapier_world::{CharacterController, PhysicsWorld};
use super::step_up::{StepUpComputeResult, StepUpProbe};
use crate::world_objects::Vec3;

/// `try_step_up`'s probe over a character controller and its body; peers'
/// mirrors in `peers` count as characters.
pub struct KccStepUpProbe<'a> {
    pub world: &'a mut PhysicsWorld,
    pub controller: &'a mut CharacterController,
    pub body: RigidBodyHandle,
    pub collider: ColliderHandle,
    pub peers: &'a ColliderSync,
}

impl StepUpProbe for KccStepUpProbe<'_> {
    fn compute(&mut self, desired: Vec3) -> StepUpComputeResult {
        self.controller
            .compute_collider_movement(self.world, self.collider, desired);
        StepUpComputeResult {
            movement: self.controller.computed_movement(),
            grounded: self.controller.computed_grounded(),
        }
    }

    fn translation(&self) -> Vec3 {
        let t = self.world.bodies[self.body].translation();
        Vec3::from_rapier(t)
    }

    fn set_translation(&mut self, p: Vec3) {
        self.world.bodies[self.body].set_translation(p.to_rapier(), true);
        self.world.propagate_modified_body_positions_to_colliders();
    }

    fn last_contact_is_character(&self) -> bool {
        self.controller
            .computed_collisions()
            .iter()
            .any(|c| self.peers.peer_id_of(c.handle).is_some())
    }
}
