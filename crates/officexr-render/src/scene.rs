//! Plain-data scene input, deserialized from the same JSON shapes the TS
//! side uses (`WorldObjectKind`, `WorldObjects`, `LightingViewConfig`,
//! `BackgroundConstants`, `BakedLayoutPlacement`).

use std::collections::HashMap;

use bevy::color::{Color, Srgba};
use bevy::math::Vec3;
use bevy::prelude::Resource;
use serde::{Deserialize, Deserializer};

#[derive(Resource, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldSceneData {
    pub kinds: Vec<WorldObjectKind>,
    pub objects: WorldObjects,
    pub lighting: LightingConfig,
    pub background: BackgroundConfig,
    #[serde(default)]
    pub baked_layouts: Vec<BakedLayoutPlacement>,
    /// A flat background instead of the sky, as the studio's editor canvases draw.
    #[serde(default, deserialize_with = "opt_hex")]
    pub clear_color: Option<Color>,
}

/// An update to a view's scene. `replace` first clears its kinds and
/// objects, then `remove` drops objects by id, then `kinds` and
/// `objects.instances` are upserted by id; kinds are never dropped
/// otherwise. Every other field present replaces the scene's. A full scene
/// is a valid diff.
#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldDiff {
    #[serde(default)]
    pub replace: bool,
    pub kinds: Option<Vec<WorldObjectKind>>,
    pub objects: Option<WorldObjects>,
    #[serde(default)]
    pub remove: Vec<String>,
    pub lighting: Option<LightingConfig>,
    pub background: Option<BackgroundConfig>,
    pub baked_layouts: Option<Vec<BakedLayoutPlacement>>,
    /// `null` brings the sky back.
    #[serde(default, deserialize_with = "some_opt_hex")]
    pub clear_color: Option<Option<Color>>,
}

impl WorldDiff {
    /// `None` when there is no scene yet and the diff lacks lighting or background.
    pub fn apply(self, scene: Option<WorldSceneData>) -> Option<WorldSceneData> {
        let mut scene = match (scene, self.lighting, self.background) {
            (Some(mut scene), lighting, background) => {
                scene.lighting = lighting.unwrap_or(scene.lighting);
                scene.background = background.unwrap_or(scene.background);
                scene
            }
            (None, Some(lighting), Some(background)) => WorldSceneData {
                kinds: vec![],
                objects: WorldObjects { instances: vec![] },
                lighting,
                background,
                baked_layouts: vec![],
                clear_color: None,
            },
            (None, ..) => return None,
        };
        if self.replace {
            scene.kinds.clear();
            scene.objects.instances.clear();
        }
        upsert(&mut scene.kinds, self.kinds, |k| &k.id);
        let instances = &mut scene.objects.instances;
        instances.retain(|i| !self.remove.contains(&i.id));
        upsert(instances, self.objects.map(|o| o.instances), |i| &i.id);
        if let Some(layouts) = self.baked_layouts {
            scene.baked_layouts = layouts;
        }
        if let Some(clear_color) = self.clear_color {
            scene.clear_color = clear_color;
        }
        Some(scene)
    }
}

fn upsert<T>(items: &mut Vec<T>, new: Option<Vec<T>>, id: impl Fn(&T) -> &String) {
    let mut at: HashMap<String, usize> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (id(item).clone(), i))
        .collect();
    for item in new.into_iter().flatten() {
        match at.get(id(&item)) {
            Some(&i) => items[i] = item,
            None => {
                at.insert(id(&item).clone(), items.len());
                items.push(item);
            }
        }
    }
}

/// One room placement of a baked layout GLB.
#[derive(Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BakedLayoutPlacement {
    pub gltf_path: String,
    /// Voxels.
    pub position: [f32; 3],
    /// Quarter turns.
    pub rotation_y: i32,
}

#[derive(Deserialize, Clone, PartialEq, Debug)]
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

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Dimensions {
    pub width: f32,
    pub height: f32,
    pub depth: f32,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct LocalAabb {
    pub min: Xyz,
    pub max: Xyz,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
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

#[derive(Deserialize, Clone, PartialEq, Debug)]
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

#[derive(Deserialize, Clone, PartialEq, Debug)]
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

#[derive(Deserialize, Clone, PartialEq, Debug)]
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

fn some_opt_hex<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<Color>>, D::Error> {
    opt_hex(d).map(Some)
}

fn opt_hex<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Color>, D::Error> {
    Option::<String>::deserialize(d)?
        .map(|s| parse_hex(&s))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_baked_layout_placements() {
        let scene: WorldSceneData =
            serde_json::from_str(include_str!("../examples/booper.scene.json"))
                .and_then(|mut v: serde_json::Value| {
                    v["bakedLayouts"] = serde_json::json!([
                        {"gltfPath": "/baked-layouts/a.glb", "position": [2, 0, 4], "rotationY": 1}
                    ]);
                    serde_json::from_value(v)
                })
                .unwrap();
        assert_eq!(scene.baked_layouts[0].gltf_path, "/baked-layouts/a.glb");
        assert_eq!(scene.baked_layouts[0].rotation_y, 1);
    }

    fn diff(json: serde_json::Value) -> WorldDiff {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn diffs_upsert_and_remove_objects_by_id() {
        let full: WorldDiff =
            serde_json::from_str(include_str!("../examples/booper.scene.json")).unwrap();
        let scene = full.apply(None).unwrap();
        let first = scene.objects.instances[0].id.clone();
        let count = scene.objects.instances.len();

        let scene = diff(serde_json::json!({
            "objects": {"instances": [
                {"id": first, "kindId": "k", "position": [9, 0, 0]},
                {"id": "new", "kindId": "k", "position": [0, 0, 0]},
            ]},
            "clearColor": "#102030",
        }))
        .apply(Some(scene))
        .unwrap();
        assert_eq!(scene.objects.instances.len(), count + 1);
        assert_eq!(scene.objects.instances[0].position, [9.0, 0.0, 0.0]);
        assert!(scene.clear_color.is_some());

        let scene = diff(serde_json::json!({"remove": ["new"], "clearColor": null}))
            .apply(Some(scene))
            .unwrap();
        assert_eq!(scene.objects.instances.len(), count);
        assert_eq!(scene.clear_color, None);
    }

    #[test]
    fn a_replacing_diff_drops_the_old_objects_and_kinds() {
        let full: WorldDiff =
            serde_json::from_str(include_str!("../examples/booper.scene.json")).unwrap();
        let scene = full.apply(None).unwrap();
        assert!(!scene.kinds.is_empty());

        let scene = diff(serde_json::json!({
            "replace": true,
            "objects": {"instances": [{"id": "only", "kindId": "k", "position": [0, 0, 0]}]},
        }))
        .apply(Some(scene))
        .unwrap();
        let ids: Vec<_> = scene
            .objects
            .instances
            .iter()
            .map(|i| i.id.as_str())
            .collect();
        assert_eq!(ids, ["only"]);
        assert!(scene.kinds.is_empty());
    }

    #[test]
    fn the_first_diff_needs_lighting_and_background() {
        assert!(
            diff(serde_json::json!({"remove": []}))
                .apply(None)
                .is_none()
        );
    }
}
