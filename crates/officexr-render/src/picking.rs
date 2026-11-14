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

#[cfg(feature = "picking")]
mod pointers;
#[cfg(feature = "picking")]
pub use pointers::*;
