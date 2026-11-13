//! The world-AABB half of `app/geometry-service.ts`.

use crate::physics::rules::WorldAabb;
use crate::sim_core::SimKind;

const PRIMITIVE_KIND_PREFIX: &str = "__primitive_";
const PRIMITIVE_BLOCK_SIZE: f64 = 2.0;

pub fn normalize_yaw_deg(yaw_deg: f64) -> f64 {
    ((yaw_deg % 360.0) + 360.0) % 360.0
}

/// Local-AABB `(min, max)` of a kind.
fn local_aabb(kind_id: &str, kind: Option<&SimKind>, voxel_size: f64) -> ([f64; 3], [f64; 3]) {
    if kind_id.starts_with(PRIMITIVE_KIND_PREFIX) {
        let h = PRIMITIVE_BLOCK_SIZE / 2.0;
        return ([-h, -h, -h], [h, h, h]);
    }
    if let Some(aabb) = kind.and_then(|k| k.local_aabb.as_ref()) {
        return (
            [aabb.min.x, aabb.min.y, aabb.min.z],
            [aabb.max.x, aabb.max.y, aabb.max.z],
        );
    }
    if let Some(d) = kind.and_then(|k| k.dimensions.as_ref()) {
        return (
            [-d.width / 2.0, 0.0, -d.depth / 2.0],
            [d.width / 2.0, d.height, d.depth / 2.0],
        );
    }
    let half = voxel_size / 2.0;
    ([-half, 0.0, -half], [half, voxel_size, half])
}

/// cos and sin of a yaw, rounded so quarter turns come out exact.
fn cos_sin(yaw_deg: f64) -> (f64, f64) {
    let rad = normalize_yaw_deg(yaw_deg) * std::f64::consts::PI / 180.0;
    let round = |v: f64| (v * 1e12).round() / 1e12;
    (round(rad.cos()), round(rad.sin()))
}

/// The world AABB of a kind placed at voxel `position`, its min corner at
/// `position * voxel_size` before the yaw.
pub fn world_aabb(
    position: [f64; 3],
    kind_id: &str,
    kind: Option<&SimKind>,
    voxel_size: f64,
    yaw_deg: f64,
) -> WorldAabb {
    let (lmin, lmax) = local_aabb(kind_id, kind, voxel_size);
    let o = position.map(|p| p * voxel_size);
    let box_ = WorldAabb {
        min: o,
        max: [
            o[0] + (lmax[0] - lmin[0]),
            o[1] + (lmax[1] - lmin[1]),
            o[2] + (lmax[2] - lmin[2]),
        ],
    };
    if normalize_yaw_deg(yaw_deg) == 0.0 {
        return box_;
    }
    let cx = (box_.min[0] + box_.max[0]) / 2.0;
    let cz = (box_.min[2] + box_.max[2]) / 2.0;
    let (cos, sin) = cos_sin(yaw_deg);
    let (cos, sin) = (cos.abs(), sin.abs());
    let x = (box_.max[0] - box_.min[0]) / 2.0;
    let z = (box_.max[2] - box_.min[2]) / 2.0;
    let (hx, hz) = (x * cos + z * sin, x * sin + z * cos);
    WorldAabb {
        min: [cx - hx, box_.min[1], cz - hz],
        max: [cx + hx, box_.max[1], cz + hz],
    }
}
