//! Baked layouts' collider descriptors, read from each layout GLB's scene extras.

use bevy::gltf::{Gltf, GltfSceneExtras};
use bevy::prelude::*;
use bevy::world_serialization::WorldAsset;
use officexr_render::AssetBase;
use officexr_render::placement::VOXEL_SIZE;
use officexr_sim::{ColliderDescriptor, SimBakedColliders, Vec3 as SimVec3};
use serde::Deserialize;
use serde_json::Value;

use crate::world::{Layout, SimWorld};

/// As `BAKED_COLLIDERS_VERSION` in `baked-collider-extras.ts`; v1 (cuboids only) is still read.
pub const BAKED_COLLIDERS_VERSION: u64 = 2;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cuboid {
    center: SimVec3,
    half_extents: SimVec3,
}

#[derive(Deserialize)]
struct Trimesh {
    vertices: Vec<f64>,
    indices: Vec<u32>,
}

/// Port of `parseEmbeddedColliders`: `None` when the extras carry no colliders, a malformed
/// payload or an unknown version.
pub fn parse_embedded_colliders(extras: &Value) -> Option<Vec<ColliderDescriptor>> {
    let colliders = extras.get("officexr")?.get("colliders")?;
    let version = colliders.get("version")?.as_f64()?;
    let version = if version == 1.0 {
        1
    } else if version == BAKED_COLLIDERS_VERSION as f64 {
        BAKED_COLLIDERS_VERSION
    } else {
        warn!(
            "baked colliders version {version} is unknown (expected 1 or {BAKED_COLLIDERS_VERSION})"
        );
        return None;
    };
    let cuboids: Vec<Cuboid> = serde_json::from_value(colliders.get("cuboids")?.clone()).ok()?;
    let mut out: Vec<_> = cuboids
        .into_iter()
        .map(|c| ColliderDescriptor::Cuboid {
            center: c.center,
            half_extents: c.half_extents,
        })
        .collect();
    if version == 2 {
        let trimeshes: Vec<Trimesh> =
            serde_json::from_value(colliders.get("trimeshes")?.clone()).ok()?;
        for t in trimeshes {
            let vertex_count = t.vertices.len() / 3;
            let valid = !t.vertices.is_empty()
                && t.vertices.len() % 3 == 0
                && !t.indices.is_empty()
                && t.indices.len() % 3 == 0
                && t.indices.iter().all(|&i| (i as usize) < vertex_count);
            if !valid {
                return None;
            }
            out.push(ColliderDescriptor::Trimesh {
                vertices: t.vertices,
                indices: t.indices,
            });
        }
    }
    Some(out)
}

/// Every layout's colliders once all have loaded or failed; layouts whose GLB failed or carried
/// no usable descriptors are left out, so their room's world objects collide instead.
#[derive(Resource)]
pub struct BakedColliders(pub Vec<SimBakedColliders>);

type PendingLayouts = Vec<(Layout, Handle<Gltf>)>;

pub fn load_baked_colliders(
    mut commands: Commands,
    world: Option<Res<SimWorld>>,
    base: Res<AssetBase>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    scenes: Res<Assets<WorldAsset>>,
    mut pending: Local<Option<PendingLayouts>>,
) {
    let Some(world) = world else {
        return;
    };
    if world.is_changed() {
        commands.remove_resource::<BakedColliders>();
        *pending = Some(
            world
                .layouts
                .iter()
                .map(|l| {
                    let handle = assets.load(base.asset_path(&l.placement.gltf_path));
                    (l.clone(), handle)
                })
                .collect(),
        );
    }
    let Some(layouts) = pending.as_ref() else {
        return;
    };
    let settled = |h: &Handle<Gltf>| {
        assets.load_state(h).is_failed() || assets.is_loaded_with_dependencies(h)
    };
    if !layouts.iter().all(|(_, h)| settled(h)) {
        return;
    }
    let baked = layouts
        .iter()
        .filter_map(|(layout, handle)| {
            let path = &layout.placement.gltf_path;
            let extras = gltfs
                .get(handle)
                .and_then(|g| g.default_scene.as_ref().or(g.scenes.first()))
                .and_then(|s| scenes.get(s))
                .and_then(|s| {
                    s.world
                        .try_query::<&GltfSceneExtras>()?
                        .iter(&s.world)
                        .next()
                        .cloned()
                });
            let colliders = extras
                .and_then(|e| serde_json::from_str(&e.value).ok())
                .and_then(|v: Value| parse_embedded_colliders(&v));
            let Some(colliders) = colliders else {
                warn!("{path}: no usable baked colliders; falling back to its world objects");
                return None;
            };
            let p = &layout.placement;
            Some(SimBakedColliders {
                room_instance_id: layout.room_instance_id.clone(),
                position: SimVec3 {
                    x: f64::from(p.position[0] * VOXEL_SIZE),
                    y: f64::from(p.position[1] * VOXEL_SIZE),
                    z: f64::from(p.position[2] * VOXEL_SIZE),
                },
                yaw_rad: f64::from(p.rotation_y) * std::f64::consts::FRAC_PI_2,
                colliders,
            })
        })
        .collect();
    *pending = None;
    commands.insert_resource(BakedColliders(baked));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn extras(version: u64) -> Value {
        json!({ "officexr": { "bakeSource": {}, "colliders": {
            "version": version,
            "cuboids": [{ "center": { "x": 0, "y": 0.5, "z": 1 }, "halfExtents": { "x": 1, "y": 0.5, "z": 1 } }],
            "trimeshes": [{ "vertices": [0, 0, 0, 1, 0, 0, 0, 0, 1], "indices": [0, 1, 2] }],
        }}})
    }

    #[test]
    fn reads_the_current_version_and_v1() {
        let v2 = parse_embedded_colliders(&extras(BAKED_COLLIDERS_VERSION)).unwrap();
        assert_eq!(v2.len(), 2);
        assert!(matches!(&v2[0], ColliderDescriptor::Cuboid { center, .. } if center.z == 1.0));
        assert!(
            matches!(&v2[1], ColliderDescriptor::Trimesh { indices, .. } if indices == &[0, 1, 2])
        );
        // v1 predates trimeshes, so they're ignored.
        assert_eq!(parse_embedded_colliders(&extras(1)).unwrap().len(), 1);
    }

    #[test]
    fn refuses_unknown_versions_and_malformed_payloads() {
        assert_eq!(
            parse_embedded_colliders(&extras(BAKED_COLLIDERS_VERSION + 1)),
            None
        );
        assert_eq!(parse_embedded_colliders(&json!({})), None);
        let mut float = extras(2);
        float["officexr"]["colliders"]["version"] = json!(2.0);
        assert_eq!(parse_embedded_colliders(&float).map(|c| c.len()), Some(2));
        let mut bad = extras(2);
        bad["officexr"]["colliders"]["trimeshes"][0]["indices"] = json!([0, 1, 3]);
        assert_eq!(parse_embedded_colliders(&bad), None);
    }
}
