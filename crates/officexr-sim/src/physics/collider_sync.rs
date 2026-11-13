use std::collections::{HashMap, HashSet};

use rapier3d::na::{Quaternion, Translation3, Unit};
use rapier3d::prelude::*;

use super::character_movement::{PeerBump, XZ};
use super::groups::{BODY_GROUPS, WALL_GROUPS};
use super::rapier_world::{CharacterController, PhysicsWorld};
use super::rules::{BODY_Y, ColliderDescriptor};
use crate::world_objects::Vec3;

/// Colliders sharing one fixed body, placed by `position` and a yaw about +Y.
pub struct StaticColliderSet<'a> {
    pub colliders: &'a [ColliderDescriptor],
    pub position: Option<Vec3>,
    pub yaw_rad: f64,
}

/// Keeps a world's static map colliders and per-peer kinematic mirror
/// bodies in step with plain-data descriptors.
#[derive(Default)]
pub struct ColliderSync {
    static_bodies: Vec<RigidBodyHandle>,
    /// In insertion order, like the TS `Map`, so handle reuse matches it.
    mirrors: Vec<(String, ColliderHandle)>,
    peer_by_handle: HashMap<ColliderHandle, String>,
}

impl ColliderSync {
    /// Replaces every static collider with `sets`.
    pub fn set_static(&mut self, world: &mut PhysicsWorld, sets: &[StaticColliderSet]) {
        for b in self.static_bodies.drain(..) {
            world.remove_rigid_body(b);
        }
        self.static_bodies = sets.iter().map(|s| add_static(world, s)).collect();
    }

    /// Creates, moves (as of the next world step) and removes one ball
    /// mirror per peer. Returns the ids whose mirrors were removed.
    pub fn sync_peers(
        &mut self,
        world: &mut PhysicsWorld,
        peers: &[(String, Vec3)],
        char_radius: f64,
    ) -> Vec<String> {
        for (id, pos) in peers {
            if let Some((_, collider)) = self.mirrors.iter().find(|(m, _)| m == id) {
                let co = &world.colliders[*collider];
                let body = co.parent().expect("mirror colliders have a body");
                let radius = co.shape().as_ball().expect("mirrors are balls").radius;
                world.bodies[body].set_next_kinematic_translation(pos.to_rapier());
                if radius != char_radius as Real {
                    set_ball_radius(world, *collider, char_radius);
                }
                continue;
            }
            let body = world
                .bodies
                .insert(RigidBodyBuilder::kinematic_position_based().translation(pos.to_rapier()));
            let collider = world.insert_collider(
                ColliderBuilder::ball(char_radius as Real)
                    .translation(vector![0.0, BODY_Y as Real, 0.0])
                    .collision_groups(BODY_GROUPS)
                    .active_events(ActiveEvents::COLLISION_EVENTS)
                    .active_collision_types(ActiveCollisionTypes::all()),
                body,
            );
            self.mirrors.push((id.clone(), collider));
            self.peer_by_handle.insert(collider, id.clone());
        }
        let (kept, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut self.mirrors)
            .into_iter()
            .partition(|(id, _)| peers.iter().any(|(p, _)| p == id));
        self.mirrors = kept;
        gone.into_iter()
            .map(|(id, collider)| {
                self.peer_by_handle.remove(&collider);
                let body = world.colliders[collider]
                    .parent()
                    .expect("mirror colliders have a body");
                world.remove_rigid_body(body);
                id
            })
            .collect()
    }

    /// Peers the controller's last sweep touched that `previous` didn't (it
    /// slides, so no contact events fire), and every peer it touched.
    pub fn edge_triggered_bumps(
        &self,
        controller: &CharacterController,
        previous: &HashSet<String>,
    ) -> (HashSet<String>, Vec<PeerBump>) {
        let mut touching = HashSet::new();
        let mut bumps = vec![];
        for c in controller.computed_collisions() {
            let Some(other_id) = self.peer_id_of(c.handle) else {
                continue;
            };
            touching.insert(other_id.clone());
            if !previous.contains(other_id) {
                bumps.push(PeerBump {
                    other_id: other_id.clone(),
                    normal: XZ {
                        x: f64::from(c.hit.normal1.x),
                        z: f64::from(c.hit.normal1.z),
                    },
                });
            }
        }
        (touching, bumps)
    }

    pub fn peer_id_of(&self, handle: ColliderHandle) -> Option<&String> {
        self.peer_by_handle.get(&handle)
    }
}

pub fn set_ball_radius(world: &mut PhysicsWorld, collider: ColliderHandle, radius: f64) {
    if let Some(ball) = world.colliders[collider].shape_mut().as_ball_mut() {
        ball.radius = radius as Real;
    }
}

fn add_static(world: &mut PhysicsWorld, set: &StaticColliderSet) -> RigidBodyHandle {
    let mut desc = RigidBodyBuilder::fixed();
    if let Some(p) = set.position {
        desc = desc.translation(p.to_rapier());
    }
    if set.yaw_rad != 0.0 {
        let half = set.yaw_rad / 2.0;
        let rotation = Unit::new_normalize(Quaternion::new(
            half.cos() as Real,
            0.0,
            half.sin() as Real,
            0.0,
        ));
        let translation = desc.position.translation;
        desc = desc.pose(Isometry::from_parts(
            Translation3::from(translation.vector),
            rotation,
        ));
    }
    let body = world.bodies.insert(desc);
    for c in set.colliders {
        let desc = match c {
            ColliderDescriptor::Cuboid {
                center,
                half_extents,
            } => ColliderBuilder::cuboid(
                half_extents.x as Real,
                half_extents.y as Real,
                half_extents.z as Real,
            )
            .translation(center.to_rapier()),
            ColliderDescriptor::Trimesh { vertices, indices } => ColliderBuilder::trimesh(
                vertices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|v| point![v[0] as Real, v[1] as Real, v[2] as Real])
                    .collect(),
                indices.as_chunks::<3>().0.to_vec(),
            )
            .expect("baked and kind trimeshes are validated upstream"),
        };
        world.insert_collider(desc.collision_groups(WALL_GROUPS), body);
    }
    body
}
