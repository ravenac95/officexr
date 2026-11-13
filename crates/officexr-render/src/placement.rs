//! Ports of `geometry-service.ts`'s `meshTransform` and `placement-transform.ts`.

use bevy::math::{Quat, Vec3};
use bevy::transform::components::Transform;

use crate::scene::{BakedLayoutPlacement, ObjectInstance, WorldObjectKind};

pub const VOXEL_SIZE: f32 = 0.5;

/// Local AABB of the kind's mesh: baked `localAABB`, else `dimensions`
/// assumed X/Z-centred and Y-bottom, else one voxel.
fn local_aabb(kind: &WorldObjectKind) -> (Vec3, Vec3) {
    if let Some(b) = kind.local_aabb {
        return (b.min.into(), b.max.into());
    }
    if let Some(d) = kind.dimensions {
        return (
            Vec3::new(-d.width / 2.0, 0.0, -d.depth / 2.0),
            Vec3::new(d.width / 2.0, d.height, d.depth / 2.0),
        );
    }
    let half = VOXEL_SIZE / 2.0;
    (
        Vec3::new(-half, 0.0, -half),
        Vec3::new(half, VOXEL_SIZE, half),
    )
}

fn normalize_yaw_deg(yaw_deg: f64) -> f64 {
    yaw_deg.rem_euclid(360.0)
}

/// cos and sin rounded so quarter turns come out exact.
fn cos_sin(yaw_rad: f64) -> (f64, f64) {
    let round = |v: f64| (v * 1e12).round() / 1e12;
    (round(yaw_rad.cos()), round(yaw_rad.sin()))
}

/// The instance's mesh root pose, matching three.js `ObjectInstances`.
pub fn mesh_transform(inst: &ObjectInstance, kind: &WorldObjectKind) -> Transform {
    let (min, max) = local_aabb(kind);
    let origin = Vec3::from(inst.position) * VOXEL_SIZE - min;
    let centre_x = origin.x + (min.x + max.x) / 2.0;
    let centre_z = origin.z + (min.z + max.z) / 2.0;
    let yaw_rad = normalize_yaw_deg(inst.yaw_deg as f64).to_radians();
    let (cos, sin) = cos_sin(yaw_rad);
    let dx = (origin.x - centre_x) as f64;
    let dz = (origin.z - centre_z) as f64;
    let translation = Vec3::new(
        centre_x + (dx * cos + dz * sin) as f32,
        origin.y,
        centre_z + (-dx * sin + dz * cos) as f32,
    );
    Transform {
        translation,
        rotation: Quat::from_rotation_y(yaw_rad as f32),
        scale: Vec3::splat(kind.scale),
    }
}

pub fn baked_layout_transform(p: &BakedLayoutPlacement) -> Transform {
    Transform {
        translation: Vec3::from(p.position) * VOXEL_SIZE,
        rotation: Quat::from_rotation_y(p.rotation_y as f32 * std::f32::consts::FRAC_PI_2),
        scale: Vec3::ONE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{LocalAabb, Xyz};

    fn kind(min: [f32; 3], max: [f32; 3]) -> WorldObjectKind {
        let xyz = |v: [f32; 3]| Xyz {
            x: v[0],
            y: v[1],
            z: v[2],
        };
        WorldObjectKind {
            id: "k".into(),
            gltf_path: "/k.gltf".into(),
            scale: 1.0,
            tint: None,
            opacity: 1.0,
            roughness: None,
            metalness: None,
            emissive: None,
            emissive_intensity: 0.0,
            dimensions: None,
            local_aabb: Some(LocalAabb {
                min: xyz(min),
                max: xyz(max),
            }),
        }
    }

    fn inst(position: [f32; 3], yaw_deg: f32) -> ObjectInstance {
        ObjectInstance {
            id: "i".into(),
            kind_id: "k".into(),
            position,
            yaw_deg,
        }
    }

    #[test]
    fn centred_block_anchors_its_min_corner_at_the_voxel() {
        let t = mesh_transform(&inst([4.0, 0.0, 0.0], 0.0), &kind([-1.0; 3], [1.0; 3]));
        assert_eq!(t.translation, Vec3::new(3.0, 1.0, 1.0));
    }

    #[test]
    fn off_centre_mesh_yaws_about_its_footprint_centre() {
        // A 4x1 wall whose mesh origin sits at its west end: the footprint
        // centre stays put while the root swings round it.
        let t = mesh_transform(
            &inst([0.0; 3], 90.0),
            &kind([0.0, 0.0, 0.0], [4.0, 1.0, 1.0]),
        );
        assert_eq!(t.translation, Vec3::new(1.5, 0.0, 2.5));
        assert!(
            t.rotation
                .angle_between(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
                < 1e-3
        );
    }

    #[test]
    fn negative_yaw_normalizes() {
        let a = mesh_transform(&inst([0.0; 3], -90.0), &kind([0.0; 3], [4.0, 1.0, 1.0]));
        let b = mesh_transform(&inst([0.0; 3], 270.0), &kind([0.0; 3], [4.0, 1.0, 1.0]));
        assert_eq!(a.translation, b.translation);
    }

    #[test]
    fn baked_layout_places_in_voxels_and_quarter_turns() {
        let t = baked_layout_transform(&BakedLayoutPlacement {
            gltf_path: "/l.glb".into(),
            position: [2.0, 0.0, 4.0],
            rotation_y: 1,
        });
        assert_eq!(t.translation, Vec3::new(1.0, 0.0, 2.0));
        assert!(
            t.rotation
                .angle_between(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
                < 1e-3
        );
    }
}
