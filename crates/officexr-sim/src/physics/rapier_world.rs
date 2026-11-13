//! The slice of rapier.js's `World` and `KinematicCharacterController`
//! wrappers the sim uses, so calls line up one-to-one with the TS code.

use rapier3d::control::{
    CharacterCollision, CharacterLength, EffectiveCharacterMovement, KinematicCharacterController,
};
use rapier3d::prelude::*;

use crate::world_objects::Vec3;

pub struct PhysicsWorld {
    pub bodies: RigidBodySet,
    pub colliders: ColliderSet,
    gravity: Vector<Real>,
    params: IntegrationParameters,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad_phase: BroadPhaseBvh,
    narrow_phase: NarrowPhase,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    ccd_solver: CCDSolver,
}

impl PhysicsWorld {
    pub fn new(gravity_y: f64) -> Self {
        Self {
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            gravity: vector![0.0, gravity_y as Real, 0.0],
            params: IntegrationParameters::default(),
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
        }
    }

    pub fn step(&mut self) {
        self.pipeline.step(
            &self.gravity,
            &self.params,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.ccd_solver,
            &(),
            &(),
        );
    }

    pub fn insert_collider(
        &mut self,
        collider: ColliderBuilder,
        parent: RigidBodyHandle,
    ) -> ColliderHandle {
        self.colliders
            .insert_with_parent(collider, parent, &mut self.bodies)
    }

    pub fn remove_rigid_body(&mut self, handle: RigidBodyHandle) {
        self.bodies.remove(
            handle,
            &mut self.islands,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            true,
        );
    }

    pub fn propagate_modified_body_positions_to_colliders(&mut self) {
        self.bodies
            .propagate_modified_body_positions_to_colliders(&mut self.colliders);
    }

    fn query_pipeline<'a>(&'a self, filter: QueryFilter<'a>) -> QueryPipeline<'a> {
        self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            filter,
        )
    }

    /// Whether a solid ray from `origin` hits a non-sensor within `max_toi`.
    pub fn cast_ray_hits(&self, origin: Vec3, dir: Vector<Real>, max_toi: f64) -> bool {
        let ray = Ray::new(origin.to_rapier().into(), dir);
        self.query_pipeline(QueryFilter::default().exclude_sensors())
            .cast_ray(&ray, max_toi as Real, true)
            .is_some()
    }
}

/// rapier.js's `KinematicCharacterController`: the controller plus the
/// result and collisions of its last `compute_collider_movement`.
pub struct CharacterController {
    pub controller: KinematicCharacterController,
    result: EffectiveCharacterMovement,
    collisions: Vec<CharacterCollision>,
}

impl CharacterController {
    pub fn new(offset: Real) -> Self {
        Self {
            controller: KinematicCharacterController {
                offset: CharacterLength::Absolute(offset),
                autostep: None,
                snap_to_ground: None,
                ..KinematicCharacterController::default()
            },
            result: EffectiveCharacterMovement {
                translation: Vector::zeros(),
                grounded: false,
                is_sliding_down_slope: false,
            },
            collisions: vec![],
        }
    }

    /// Sweeps `collider` by `desired`, ignoring sensors and its own body.
    pub fn compute_collider_movement(
        &mut self,
        world: &PhysicsWorld,
        collider: ColliderHandle,
        desired: Vec3,
    ) {
        let co = &world.colliders[collider];
        let filter = QueryFilter {
            exclude_collider: Some(collider),
            exclude_rigid_body: co.parent(),
            ..QueryFilter::default().exclude_sensors()
        };
        self.collisions.clear();
        let collisions = &mut self.collisions;
        self.result = self.controller.move_shape(
            world.params.dt,
            &world.query_pipeline(filter),
            co.shape(),
            co.position(),
            desired.to_rapier(),
            |c| collisions.push(c),
        );
    }

    pub fn computed_movement(&self) -> Vec3 {
        Vec3::from_rapier(&self.result.translation)
    }

    pub fn computed_grounded(&self) -> bool {
        self.result.grounded
    }

    pub fn computed_collisions(&self) -> &[CharacterCollision] {
        &self.collisions
    }
}
