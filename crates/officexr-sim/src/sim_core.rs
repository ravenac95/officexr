use std::collections::HashSet;

use rapier3d::control::CharacterLength;
use rapier3d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::physics::air_control::step_air_velocity;
use crate::physics::blocking::horizontal_progress;
use crate::physics::character_movement::{AnimState, PeerBump, XZ};
use crate::physics::collider_sync::{
    ColliderSync, InvalidTrimesh, StaticColliderSet, set_ball_radius,
};
use crate::physics::groups::BODY_GROUPS;
use crate::physics::kcc_step_up_probe::KccStepUpProbe;
use crate::physics::rapier_world::{CharacterController, PhysicsWorld};
use crate::physics::rules::{
    BODY_Y, CHARACTER_CONTROLLER_SKIN, ColliderDescriptor, ColliderShapeSpec, FLOOR_PROBE_RANGE,
    GRAVITY, pick_respawn_position, respawn_threshold, should_respawn_falling,
    world_objects_to_colliders,
};
use crate::physics::step_up::try_step_up;
use crate::proximity::{PairTracker, ProximityInput, TrackerOutput};
use crate::world_objects::{Vec3, WorldObjects};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimKinds {
    pub voxel_size: f64,
    pub kinds: Vec<SimKind>,
}

/// The kind fields static colliders derive from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimKind {
    pub id: String,
    #[serde(rename = "localAABB", default, skip_serializing_if = "Option::is_none")]
    pub local_aabb: Option<LocalAabb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<Dimensions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collider_shape: Option<ColliderShapeSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalAabb {
    pub min: Vec3,
    pub max: Vec3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimensions {
    pub width: f64,
    pub height: f64,
    pub depth: f64,
}

/// A baked layout's layout-local colliders and its placement. Its room's
/// instances collide through these instead, also while `colliders` is
/// still empty because the layout hasn't loaded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimBakedColliders {
    pub room_instance_id: String,
    pub position: Vec3,
    pub yaw_rad: f64,
    pub colliders: Vec<ColliderDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimPeer {
    pub id: String,
    /// Body-root position this frame; the mirror moves there for the next step.
    pub pos: Vec3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimSettings {
    pub char_radius: f64,
    pub movement_block_threshold: f64,
    pub jump_velocity: f64,
    pub air_control: f64,
    pub max_jumps: u32,
    pub landing_ease_ms: f64,
}

/// One frame of the local player's intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimInput {
    pub dt_sec: f64,
    pub now_ms: f64,
    /// World-space unit XZ direction, or zero for none.
    pub dir: XZ,
    pub speed: f64,
    /// Only picks the `anim_state` of a successful move.
    pub running: bool,
    pub yaw: f64,
    /// Pressed this frame (edge, not held).
    pub jump: bool,
    pub settings: SimSettings,
    /// Where a fallen player respawns; empty means never.
    pub spawn_points: Vec<Vec3>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimStepResult {
    pub new_pos: Vec3,
    pub broadcast_vel: Vec3,
    pub broadcast_yaw: f64,
    pub moved: bool,
    pub anim_state: AnimState,
    /// Integrated vertical velocity, negative falling.
    pub vel_y: f64,
    pub has_floor_underneath: bool,
    pub is_grounded: bool,
    /// Edge-triggered: only peers not touched the step before.
    pub bumps: Vec<PeerBump>,
    /// Airborne as of the start of the step: what the move broadcasts and animates.
    pub is_airborne: bool,
    /// The player fell off the map and `new_pos` is a spawn point.
    pub respawned: bool,
}

/// The local player's movement simulation: a kinematic ball the character
/// controller sweeps, with gravity, jumps, air control and the landing ease
/// integrated here because the controller only resolves the translation
/// it is given.
pub struct SimCore {
    world: PhysicsWorld,
    colliders: ColliderSync,
    body: RigidBodyHandle,
    body_collider: ColliderHandle,
    controller: CharacterController,
    world_objects: WorldObjects,
    gravity_enabled: bool,
    vertical_vel: f64,
    /// Set by a jump and cleared once the landing ease finishes; not inferred from jumps left.
    airborne: bool,
    /// Refilled to `max_jumps` on the ground; starts empty until the first grounded step.
    jumps_remaining: u32,
    max_jumps: u32,
    air_vel: (f64, f64),
    landing_blend_ms: f64,
    blend_start_vel: (f64, f64),
    bumping_peers: HashSet<String>,
    pair_tracker: PairTracker,
}

impl SimCore {
    pub fn new(char_radius: f64) -> Self {
        let mut world = PhysicsWorld::new(GRAVITY);
        let body = world
            .bodies
            .insert(RigidBodyBuilder::kinematic_position_based());
        let body_collider = world.insert_collider(
            ColliderBuilder::ball(char_radius as Real)
                .translation(vector![0.0, BODY_Y as Real, 0.0])
                .collision_groups(BODY_GROUPS)
                .active_collision_types(ActiveCollisionTypes::all()),
            body,
        );
        let mut controller = CharacterController::new(CHARACTER_CONTROLLER_SKIN);
        controller.controller.slide = true;
        controller.controller.up = Vector::y_axis();
        // Keeps the ball on the ground across seams between nominally level cubes.
        controller.controller.snap_to_ground = Some(CharacterLength::Absolute(0.3));
        // No Rapier autostep: it can't lift the ball over real risers yet
        // climbs other characters; `try_step_up` climbs ledges instead.
        Self {
            world,
            colliders: ColliderSync::default(),
            body,
            body_collider,
            controller,
            world_objects: WorldObjects {
                cube_size: 1.0,
                instances: vec![],
            },
            gravity_enabled: true,
            vertical_vel: 0.0,
            airborne: false,
            jumps_remaining: 0,
            max_jumps: 0,
            air_vel: (0.0, 0.0),
            landing_blend_ms: 0.0,
            blend_start_vel: (0.0, 0.0),
            bumping_peers: HashSet::new(),
            pair_tracker: PairTracker::default(),
        }
    }

    /// Advances proximity pairing and MeetingAreas one frame over `players`,
    /// in the caller's player order.
    pub fn tick_proximity(&mut self, players: &[SimPeer], input: &ProximityInput) -> TrackerOutput {
        self.pair_tracker.tick(players, input)
    }

    pub fn set_static_colliders(
        &mut self,
        world_objects: &WorldObjects,
        kinds: &SimKinds,
        baked_colliders: &[SimBakedColliders],
    ) -> Result<(), InvalidTrimesh> {
        let layout_prefixes: Vec<String> = baked_colliders
            .iter()
            .map(|b| format!("{}/layout/", b.room_instance_id))
            .collect();
        let unbaked = WorldObjects {
            cube_size: world_objects.cube_size,
            instances: world_objects
                .instances
                .iter()
                .filter(|i| !layout_prefixes.iter().any(|p| i.id.starts_with(p)))
                .cloned()
                .collect(),
        };
        let unbaked_colliders = world_objects_to_colliders(&unbaked, kinds);
        let sets: Vec<StaticColliderSet> = std::iter::once(StaticColliderSet {
            colliders: &unbaked_colliders,
            position: None,
            yaw_rad: 0.0,
        })
        .chain(baked_colliders.iter().map(|b| StaticColliderSet {
            colliders: &b.colliders,
            position: Some(b.position),
            yaw_rad: b.yaw_rad,
        }))
        .collect();
        self.colliders.set_static(&mut self.world, &sets)?;
        self.world_objects = world_objects.clone();
        self.world.step();
        Ok(())
    }

    /// Peers absent from `peers` lose their mirrors.
    pub fn sync_peers(&mut self, peers: &[SimPeer], char_radius: f64) {
        let peers: Vec<(String, Vec3)> = peers.iter().map(|p| (p.id.clone(), p.pos)).collect();
        for id in self
            .colliders
            .sync_peers(&mut self.world, &peers, char_radius)
        {
            self.bumping_peers.remove(&id);
        }
    }

    /// The player's body-root position as of the last step or teleport.
    pub fn position(&self) -> Vec3 {
        let t = self.world.bodies[self.body].translation();
        Vec3::from_rapier(t)
    }

    /// Moves the player to `pos` and clears its fall and jump state.
    pub fn teleport(&mut self, pos: Vec3) {
        self.world.bodies[self.body].set_next_kinematic_translation(pos.to_rapier());
        self.world.step();
        self.land();
    }

    /// Grounded with a fresh set of jumps and no fall, carry or landing ease left.
    fn land(&mut self) {
        self.vertical_vel = 0.0;
        self.airborne = false;
        self.jumps_remaining = self.max_jumps;
        self.air_vel = (0.0, 0.0);
        self.landing_blend_ms = 0.0;
    }

    /// Off pins the player: no gravity and no jumping.
    pub fn set_gravity_enabled(&mut self, on: bool) {
        self.gravity_enabled = on;
        if !on {
            // Jumps left stay: zeroing them would read as airborne every frame.
            self.vertical_vel = 0.0;
            self.airborne = false;
            self.air_vel = (0.0, 0.0);
            self.landing_blend_ms = 0.0;
        }
    }

    pub fn step(&mut self, input: &SimInput) -> SimStepResult {
        let SimInput {
            dt_sec, dir, speed, ..
        } = *input;
        let s = &input.settings;
        self.max_jumps = s.max_jumps;
        let radius = self.world.colliders[self.body_collider]
            .shape()
            .as_ball()
            .expect("the body is a ball")
            .radius;
        if radius != s.char_radius as Real {
            set_ball_radius(&mut self.world, self.body_collider, s.char_radius);
        }
        let is_airborne = self.airborne;
        let intends_move = dir.x != 0.0 || dir.z != 0.0;
        let intent_vel = (dir.x * speed, dir.z * speed);

        self.vertical_vel = if self.gravity_enabled {
            self.vertical_vel + GRAVITY * dt_sec
        } else {
            0.0
        };

        // Reads the previous step's sweep, so it runs before the jump: a
        // freshly landed step that also jumps gets its jump back.
        if self.controller.computed_grounded() {
            self.vertical_vel = 0.0;
            if self.airborne {
                if self.landing_blend_ms == 0.0 {
                    self.blend_start_vel = self.air_vel;
                }
                self.landing_blend_ms += dt_sec * 1000.0;
                if s.landing_ease_ms <= 0.0 || self.landing_blend_ms >= s.landing_ease_ms {
                    self.land();
                } else {
                    let keep = 1.0 - self.landing_blend_ms / s.landing_ease_ms;
                    self.air_vel = (self.blend_start_vel.0 * keep, self.blend_start_vel.1 * keep);
                }
            } else {
                self.jumps_remaining = s.max_jumps;
                self.landing_blend_ms = 0.0;
            }
        }

        if input.jump && self.jumps_remaining > 0 && self.gravity_enabled {
            self.vertical_vel = s.jump_velocity;
            if !self.airborne {
                self.air_vel = intent_vel;
            }
            self.airborne = true;
            self.jumps_remaining -= 1;
            self.landing_blend_ms = 0.0;
        }

        if is_airborne && self.airborne && self.landing_blend_ms == 0.0 {
            self.air_vel = step_air_velocity(self.air_vel, intent_vel, s.air_control, dt_sec);
        }
        let (move_x, move_z) = if is_airborne {
            (self.air_vel.0 * dt_sec, self.air_vel.1 * dt_sec)
        } else {
            (intent_vel.0 * dt_sec, intent_vel.1 * dt_sec)
        };

        let corrected = self.sweep(move_x, move_z, dt_sec);

        // Lifted over a ledge rather than falling. Only a falling speed
        // resets: a jump's ascent also moves up and must survive.
        if self.vertical_vel < 0.0 && corrected.y >= 0.0 {
            self.vertical_vel = 0.0;
        }

        let (touching, bumps) = self
            .colliders
            .edge_triggered_bumps(&self.controller, &self.bumping_peers);
        self.bumping_peers = touching;

        let progress = horizontal_progress((corrected.x, corrected.z), (move_x, move_z));
        let mut horiz_blocked = intends_move && progress < 1.0 - s.movement_block_threshold;
        let mut delta = corrected;

        // Characters are not stairs: blocked by a peer, bump instead of climbing it.
        if horiz_blocked
            && self.bumping_peers.is_empty()
            && self.controller.computed_grounded()
            && self.vertical_vel <= 0.0
        {
            let mut probe = KccStepUpProbe {
                world: &mut self.world,
                controller: &mut self.controller,
                body: self.body,
                collider: self.body_collider,
                peers: &self.colliders,
            };
            if let Some(hop) = try_step_up(&mut probe, move_x, move_z) {
                delta = hop;
                horiz_blocked = false;
                self.vertical_vel = 0.0;
            }
            // The probe sweeps overwrote the controller state the next step's grounded check reads.
            self.sweep(move_x, move_z, dt_sec);
        }

        let t = self.position();
        let mut new_pos = Vec3 {
            x: if horiz_blocked { t.x } else { t.x + delta.x },
            y: t.y + delta.y,
            z: if horiz_blocked { t.z } else { t.z + delta.z },
        };

        let has_floor_underneath =
            self.world
                .cast_ray_hits(new_pos, -Vector::y(), FLOOR_PROBE_RANGE);
        // The spawn floor gives baked maps, whose floors aren't instances, a finite backstop.
        let respawn_at = if should_respawn_falling(self.vertical_vel, has_floor_underneath)
            || new_pos.y < respawn_threshold(&self.world_objects, &input.spawn_points)
        {
            pick_respawn_position(&input.spawn_points)
        } else {
            None
        };
        if let Some(at) = respawn_at {
            new_pos = at;
            self.land();
        }

        self.world.bodies[self.body].set_next_kinematic_translation(new_pos.to_rapier());
        self.world.step();

        let walking = intends_move && !horiz_blocked;
        let broadcast_vel = if is_airborne {
            Vec3 {
                x: self.air_vel.0,
                y: 0.0,
                z: self.air_vel.1,
            }
        } else if walking {
            Vec3 {
                x: dir.x * speed * progress,
                y: 0.0,
                z: dir.z * speed * progress,
            }
        } else {
            Vec3::default()
        };
        SimStepResult {
            new_pos,
            broadcast_vel,
            broadcast_yaw: input.yaw,
            moved: is_airborne || walking,
            anim_state: match (walking, input.running) {
                (false, _) => AnimState::Idle,
                (true, false) => AnimState::Walk,
                (true, true) => AnimState::Run,
            },
            vel_y: self.vertical_vel,
            has_floor_underneath,
            is_grounded: self.controller.computed_grounded(),
            bumps,
            is_airborne,
            respawned: respawn_at.is_some(),
        }
    }

    fn sweep(&mut self, move_x: f64, move_z: f64, dt_sec: f64) -> Vec3 {
        // Sensors excluded: the query pipeline would otherwise treat the
        // invisible proximity rings as solid.
        self.controller.compute_collider_movement(
            &self.world,
            self.body_collider,
            Vec3 {
                x: move_x,
                y: self.vertical_vel * dt_sec,
                z: move_z,
            },
        );
        self.controller.computed_movement()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(char_radius: f64) -> SimInput {
        SimInput {
            dt_sec: 1.0 / 60.0,
            now_ms: 0.0,
            dir: XZ::default(),
            speed: 0.0,
            running: false,
            yaw: 0.0,
            jump: false,
            settings: SimSettings {
                char_radius,
                movement_block_threshold: 0.9,
                jump_velocity: 8.0,
                air_control: 0.2,
                max_jumps: 2,
                landing_ease_ms: 120.0,
            },
            spawn_points: vec![],
        }
    }

    fn radius(sim: &SimCore) -> Real {
        sim.world.colliders[sim.body_collider]
            .shape()
            .as_ball()
            .unwrap()
            .radius
    }

    #[test]
    fn steps_resize_the_body_only_when_the_f32_radius_changes() {
        let mut sim = SimCore::new(0.4);
        sim.step(&input(0.4 + 1e-12));
        assert_eq!(radius(&sim), 0.4_f32);
        sim.step(&input(0.5));
        assert_eq!(radius(&sim), 0.5_f32);
    }
}
