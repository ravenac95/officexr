//! Bevy picking on views, fed pointer input per view through a [`PickHub`]
//! rather than from Bevy's windows: on WebGL2 a view draws into an image, so
//! its canvas's pointer never reaches Bevy. Pointer events bubble up to the
//! nearest [`PickTarget`] and leave through the hub as [`PickEvent`]s.

use bevy::prelude::*;
use serde::Serialize;

use crate::SceneCamera;

/// What a pick on this entity, or on a mesh below it, reports.
#[derive(Component, Serialize, Clone, Debug, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PickTarget {
    Object {
        id: String,
    },
    Layout {
        name: String,
        room_instance_id: String,
    },
    Player {
        id: String,
    },
    Overlay {
        tag: String,
    },
    /// The infinite floor plane at y = 0.
    Ground,
}

/// The ray through `screen`, in pixels from the top left of a view `size`
/// pixels big, as `camera` sees it.
pub fn ray_at(camera: &SceneCamera, size: Vec2, screen: Vec2) -> Option<Ray3d> {
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let ndc = screen / size * 2.0 - 1.0;
    let half_height = (camera.fov_deg.to_radians() / 2.0).tan();
    let half_width = half_height * size.x / size.y;
    let toward = Vec3::new(ndc.x * half_width, -ndc.y * half_height, -1.0);
    let transform = crate::camera_transform(camera);
    Some(Ray3d::new(
        transform.translation,
        Dir3::new(transform.rotation * toward).ok()?,
    ))
}

/// Where `ray` meets the plane `normal · p + constant = 0` (three.js's
/// `Plane`, `normal` need not be unit), or `None` when it doesn't ahead of
/// the ray's origin, or `normal` is zero.
pub fn intersect_plane(ray: Ray3d, normal: Vec3, constant: f32) -> Option<Vec3> {
    let origin = -constant * normal / normal.length_squared();
    let normal = Dir3::new(normal).ok()?;
    ray.intersect_plane(origin, InfinitePlane3d { normal })
        .map(|t| ray.get_point(t))
}

#[cfg(feature = "picking")]
mod pointers;
#[cfg(feature = "picking")]
pub use pointers::*;

#[cfg(test)]
mod tests {
    use bevy::camera::{CameraProjection, PerspectiveProjection};

    use super::*;

    fn camera(position: Vec3, look_at: Vec3, fov_deg: f32) -> SceneCamera {
        SceneCamera {
            position: position.into(),
            look_at: look_at.into(),
            fov_deg,
            near: 0.1,
            far: 100.0,
        }
    }

    fn distance_to(ray: Ray3d, point: Vec3) -> f32 {
        let along = (point - ray.origin).dot(*ray.direction);
        assert!(along > 0.0, "{point} is behind the ray");
        ray.get_point(along).distance(point)
    }

    #[test]
    fn an_off_centre_ray_passes_through_the_point_bevy_projects_there() {
        // Looking down -Z, so view space is world space moved by `position`.
        let position = Vec3::new(1.0, 2.0, 3.0);
        let cam = camera(position, position - Vec3::Z, 50.0);
        let size = Vec2::new(200.0, 100.0);
        let projection = PerspectiveProjection {
            fov: 50f32.to_radians(),
            aspect_ratio: size.x / size.y,
            ..default()
        };
        let clip_from_view = projection.get_clip_from_view();
        // Up and right of centre, so a wrong aspect, Y sign or FOV misses it.
        for in_view in [Vec3::new(1.5, 0.8, -4.0), Vec3::new(-0.3, -1.2, -6.0)] {
            let ndc = clip_from_view.project_point3(in_view);
            let screen = Vec2::new((ndc.x + 1.0) / 2.0, (1.0 - ndc.y) / 2.0) * size;
            let ray = ray_at(&cam, size, screen).unwrap();
            assert!(ray.origin.distance(position) < 1e-5);
            let miss = distance_to(ray, position + in_view);
            assert!(miss < 1e-4, "{screen}: misses {in_view} by {miss}");
        }
    }

    #[test]
    fn a_ray_meets_a_three_js_plane() {
        let cam = camera(Vec3::new(0.0, 5.0, 5.0), Vec3::ZERO, 60.0);
        let size = Vec2::new(64.0, 48.0);
        let ray = ray_at(&cam, size, size / 2.0).unwrap();
        let floor = intersect_plane(ray, Vec3::Y, 0.0).unwrap();
        assert!(floor.distance(Vec3::ZERO) < 1e-4, "{floor}");
        // y = 2, with a normal that isn't unit.
        let raised = intersect_plane(ray, Vec3::new(0.0, 2.0, 0.0), -4.0).unwrap();
        assert!(raised.distance(Vec3::new(0.0, 2.0, 2.0)) < 1e-4, "{raised}");
        // y = 10 lies behind the camera along this ray.
        assert_eq!(intersect_plane(ray, Vec3::Y, -10.0), None);
        // A plane the ray runs alongside, a metre off it.
        let parallel = ray.direction.cross(Vec3::X).normalize();
        assert_eq!(intersect_plane(ray, parallel, 1.0), None);
        assert_eq!(intersect_plane(ray, Vec3::ZERO, 0.0), None);
    }
}
