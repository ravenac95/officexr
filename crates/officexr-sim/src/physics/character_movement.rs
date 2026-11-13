use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimState {
    Idle,
    Walk,
    Run,
}

/// A peer the character first touched this step, with the contact normal pointing out of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerBump {
    pub other_id: String,
    pub normal: XZ,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct XZ {
    pub x: f64,
    pub z: f64,
}
