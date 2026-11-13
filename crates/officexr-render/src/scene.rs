//! Plain-data scene input, deserialized from the same JSON shapes the TS
//! side uses (`WorldObjectKind`, `WorldObjects`, `LightingViewConfig`,
//! `BackgroundConstants`).

use bevy::color::{Color, Srgba};
use bevy::math::Vec3;
use bevy::prelude::Resource;
use serde::{Deserialize, Deserializer};

#[derive(Resource, Deserialize, Clone, Debug)]
pub struct WorldSceneData {
    pub kinds: Vec<WorldObjectKind>,
    pub objects: WorldObjects,
    pub lighting: LightingConfig,
    pub background: BackgroundConfig,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldObjectKind {
    pub id: String,
    pub gltf_path: String,
    pub scale: f32,
    #[serde(deserialize_with = "opt_hex")]
    pub tint: Option<Color>,
    pub opacity: f32,
    pub roughness: Option<f32>,
    pub metalness: Option<f32>,
    #[serde(deserialize_with = "opt_hex")]
    pub emissive: Option<Color>,
    pub emissive_intensity: f32,
    #[serde(default)]
    pub dimensions: Option<Dimensions>,
    #[serde(default, rename = "localAABB")]
    pub local_aabb: Option<LocalAabb>,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Dimensions {
    pub width: f32,
    pub height: f32,
    pub depth: f32,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct LocalAabb {
    pub min: Xyz,
    pub max: Xyz,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Xyz {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<Xyz> for Vec3 {
    fn from(v: Xyz) -> Self {
        Vec3::new(v.x, v.y, v.z)
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct WorldObjects {
    pub instances: Vec<ObjectInstance>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInstance {
    pub id: String,
    pub kind_id: String,
    /// Voxel coordinates of the unrotated AABB's lower-left-bottom corner.
    pub position: [f32; 3],
    #[serde(default)]
    pub yaw_deg: f32,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuxLightType {
    None,
    Spot,
    Point,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LightingConfig {
    pub sun_position: [f32; 3],
    #[serde(deserialize_with = "hex")]
    pub sun_color: Color,
    pub sun_intensity: f32,
    pub ambient_intensity: f32,
    pub cast_shadow: bool,
    pub shadow_range: f32,
    pub shadow_map_size: u32,
    pub shadow_bias: f32,
    pub shadow_normal_bias: f32,
    pub aux_light_type: AuxLightType,
    pub aux_intensity: f32,
    pub aux_distance: f32,
    pub aux_angle: f32,
    pub aux_penumbra: f32,
    pub aux_decay: f32,
    pub show_sun_disc: bool,
    pub sun_disc_radius: f32,
    pub sun_disc_intensity: f32,
    #[serde(default)]
    pub ambient_fill_intensity: f32,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundConfig {
    #[serde(deserialize_with = "hex")]
    pub top_color: Color,
    #[serde(default, deserialize_with = "opt_hex")]
    pub horizon_color: Option<Color>,
    #[serde(deserialize_with = "hex")]
    pub bottom_color: Color,
}

fn parse_hex<E: serde::de::Error>(s: &str) -> Result<Color, E> {
    Srgba::hex(s)
        .map(Color::Srgba)
        .map_err(|e| E::custom(format!("bad colour {s:?}: {e}")))
}

fn hex<'de, D: Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
    parse_hex(&String::deserialize(d)?)
}

fn opt_hex<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Color>, D::Error> {
    Option::<String>::deserialize(d)?
        .map(|s| parse_hex(&s))
        .transpose()
}
