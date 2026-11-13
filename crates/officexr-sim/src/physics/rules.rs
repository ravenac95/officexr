//! Game-physics constants and collider construction every character obeys.

use serde::{Deserialize, Serialize};

use crate::geometry::{normalize_yaw_deg, world_aabb};
use crate::sim_core::SimKinds;
use crate::world_objects::{Vec3, WorldObjects};

pub const GRAVITY: f64 = -50.0;
pub const SPAWN_DROP_HEIGHT: f64 = 4.0;
pub const CHARACTER_CONTROLLER_SKIN: f32 = 0.0001;
/// Height of a character's body-ball centre above its rigid-body root.
pub const BODY_Y: f64 = 0.9;
pub const RESPAWN_MARGIN: f64 = 10.0;
pub const MAX_FALL_VELOCITY: f64 = 80.0;
pub const FLOOR_PROBE_RANGE: f64 = 2.0;

/// Both fall-respawn gates: no floor under the character and falling at least `MAX_FALL_VELOCITY`.
pub fn should_respawn_falling(vel_y: f64, has_floor_underneath: bool) -> bool {
    !has_floor_underneath && vel_y <= -MAX_FALL_VELOCITY
}

/// The y below which a character has fallen off the map: `RESPAWN_MARGIN`
/// under the lowest cube bottom or spawn point, or -inf when there are neither.
pub fn respawn_threshold(world_objects: &WorldObjects, spawns: &[Vec3]) -> f64 {
    let bottoms = world_objects
        .instances
        .iter()
        .map(|i| i.position[1] * world_objects.cube_size);
    let min = bottoms
        .chain(spawns.iter().map(|s| s.y))
        .fold(f64::INFINITY, f64::min);
    if min == f64::INFINITY {
        f64::NEG_INFINITY
    } else {
        min - RESPAWN_MARGIN
    }
}

/// The first spawn point lifted by `SPAWN_DROP_HEIGHT`, if any.
pub fn pick_respawn_position(spawns: &[Vec3]) -> Option<Vec3> {
    spawns.first().map(|s| Vec3 {
        y: s.y + SPAWN_DROP_HEIGHT,
        ..*s
    })
}

/// A world-space collider shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ColliderDescriptor {
    #[serde(rename_all = "camelCase")]
    Cuboid { center: Vec3, half_extents: Vec3 },
    Trimesh {
        vertices: Vec<f64>,
        indices: Vec<u32>,
    },
}

/// A kind's collider override, normalized 0..1 against its local AABB
/// except for `CompoundSteps`, which is in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ColliderShapeSpec {
    #[serde(rename_all = "camelCase")]
    CompoundSteps {
        step_count: u32,
        step_rise: f64,
        step_run: f64,
        step_depth: f64,
    },
    ScannedCuboids {
        cuboids: Vec<NormalizedCuboid>,
    },
    Trimesh {
        positions: Vec<f64>,
        indices: Vec<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedCuboid {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldAabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl WorldAabb {
    fn size(&self) -> [f64; 3] {
        [0, 1, 2].map(|i| self.max[i] - self.min[i])
    }

    /// `n` (0..1 per axis) mapped through this box.
    fn denormalize(&self, n: [f64; 3]) -> [f64; 3] {
        let s = self.size();
        [0, 1, 2].map(|i| self.min[i] + n[i] * s[i])
    }
}

fn cuboid_between(min: [f64; 3], max: [f64; 3]) -> ColliderDescriptor {
    ColliderDescriptor::Cuboid {
        center: Vec3 {
            x: (min[0] + max[0]) / 2.0,
            y: (min[1] + max[1]) / 2.0,
            z: (min[2] + max[2]) / 2.0,
        },
        half_extents: Vec3 {
            x: (max[0] - min[0]) / 2.0,
            y: (max[1] - min[1]) / 2.0,
            z: (max[2] - min[2]) / 2.0,
        },
    }
}

/// World-space colliders for every instance: its kind's `colliderShape`
/// when it has one and a quarter-turn yaw, otherwise its world AABB.
pub fn world_objects_to_colliders(
    world_objects: &WorldObjects,
    kinds: &SimKinds,
) -> Vec<ColliderDescriptor> {
    world_objects
        .instances
        .iter()
        .flat_map(|inst| {
            let kind = kinds.kinds.iter().find(|k| k.id == inst.kind_id);
            let yaw_deg = normalize_yaw_deg(inst.yaw_deg.unwrap_or(0.0));
            let aabb = |yaw| world_aabb(inst.position, &inst.kind_id, kind, kinds.voxel_size, yaw);
            match kind.and_then(|k| k.collider_shape.as_ref()) {
                Some(spec) if yaw_deg % 90.0 == 0.0 => {
                    let aabb = aabb(0.0);
                    let centre = (
                        (aabb.min[0] + aabb.max[0]) / 2.0,
                        (aabb.min[2] + aabb.max[2]) / 2.0,
                    );
                    shape_colliders(&aabb, spec)
                        .into_iter()
                        .map(|d| rotate_about_centre(d, centre, (yaw_deg / 90.0) as u8))
                        .collect()
                }
                // ponytail: off-quarter-turn stairs/scans/slopes collide as one box; inadequate once such a kind's rotationStepDeg isn't a multiple of 90.
                _ => {
                    let aabb = aabb(yaw_deg);
                    vec![cuboid_between(aabb.min, aabb.max)]
                }
            }
        })
        .collect()
}

fn shape_colliders(aabb: &WorldAabb, spec: &ColliderShapeSpec) -> Vec<ColliderDescriptor> {
    match spec {
        ColliderShapeSpec::CompoundSteps {
            step_count,
            step_rise,
            step_run,
            step_depth,
        } => {
            let [ox, oy, oz] = aabb.min;
            let total_width = aabb.max[0] - aabb.min[0];
            (0..*step_count)
                .map(|i| {
                    let x_far = ox + total_width - f64::from(i + 1) * step_run;
                    let y_top = oy + f64::from(i + 1) * step_rise;
                    cuboid_between([x_far, oy, oz], [x_far + step_run, y_top, oz + step_depth])
                })
                .collect()
        }
        ColliderShapeSpec::ScannedCuboids { cuboids } => cuboids
            .iter()
            .map(|c| cuboid_between(aabb.denormalize(c.min), aabb.denormalize(c.max)))
            .collect(),
        ColliderShapeSpec::Trimesh { positions, indices } => {
            vec![ColliderDescriptor::Trimesh {
                vertices: positions
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|p| aabb.denormalize(*p))
                    .collect(),
                indices: indices.clone(),
            }]
        }
    }
}

/// Turns `(x, z)` about +Y in the sense of three.js `rotation.y = quarter_turns·π/2`.
fn rotate_xz(quarter_turns: u8, x: f64, z: f64) -> (f64, f64) {
    match quarter_turns {
        0 => (x, z),
        1 => (z, -x),
        2 => (-x, -z),
        _ => (-z, x),
    }
}

fn rotate_about_centre(
    d: ColliderDescriptor,
    centre: (f64, f64),
    quarter_turns: u8,
) -> ColliderDescriptor {
    if quarter_turns == 0 {
        return d;
    }
    let rotate = |x: f64, z: f64| {
        let (rx, rz) = rotate_xz(quarter_turns, x - centre.0, z - centre.1);
        (centre.0 + rx, centre.1 + rz)
    };
    match d {
        ColliderDescriptor::Trimesh {
            mut vertices,
            indices,
        } => {
            for v in vertices.as_chunks_mut::<3>().0 {
                (v[0], v[2]) = rotate(v[0], v[2]);
            }
            ColliderDescriptor::Trimesh { vertices, indices }
        }
        ColliderDescriptor::Cuboid {
            center,
            half_extents: h,
        } => {
            let (x, z) = rotate(center.x, center.z);
            ColliderDescriptor::Cuboid {
                center: Vec3 { x, z, ..center },
                half_extents: if quarter_turns == 2 {
                    h
                } else {
                    Vec3 {
                        x: h.z,
                        y: h.y,
                        z: h.x,
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawns_only_when_both_fall_gates_hold() {
        assert!(should_respawn_falling(-MAX_FALL_VELOCITY, false));
        assert!(!should_respawn_falling(-MAX_FALL_VELOCITY, true));
        assert!(!should_respawn_falling(-MAX_FALL_VELOCITY + 1.0, false));
    }
}
