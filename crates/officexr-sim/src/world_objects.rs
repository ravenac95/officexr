use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldObjects {
    pub cube_size: f64,
    pub instances: Vec<ObjectInstance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInstance {
    pub id: String,
    pub kind_id: String,
    /// Voxel coordinates.
    pub position: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yaw_deg: Option<f64>,
}

impl Vec3 {
    /// Rapier works in `f32`; the sim's arithmetic stays in `f64` like the TS.
    pub(crate) fn to_rapier(self) -> rapier3d::prelude::Vector<f32> {
        rapier3d::prelude::Vector::new(self.x as f32, self.y as f32, self.z as f32)
    }

    pub(crate) fn from_rapier(v: &rapier3d::prelude::Vector<f32>) -> Self {
        Self {
            x: f64::from(v.x),
            y: f64::from(v.y),
            z: f64::from(v.z),
        }
    }
}
