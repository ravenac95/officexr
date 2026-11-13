//! Port of `ObjectInstances.tsx` + `cube-material.ts`: every instance of a
//! kind shares one mesh and one material, which Bevy batches into a single
//! instanced draw.

use std::collections::HashMap;

use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::lighting::{HemisphereFill, ObjectMaterial};
use crate::placement::mesh_transform;
use crate::scene::{ObjectInstance, WorldObjectKind, WorldSceneData};
use crate::{AssetBase, SceneEntity};

/// Kinds whose glTF is still loading, with their instances.
#[derive(Resource, Default)]
pub(crate) struct PendingKinds(Vec<(WorldObjectKind, Handle<Gltf>, Vec<ObjectInstance>)>);

pub(crate) fn request_kinds(
    scene: Res<WorldSceneData>,
    base: Res<AssetBase>,
    assets: Res<AssetServer>,
    mut pending: ResMut<PendingKinds>,
) {
    let kinds: HashMap<&str, &WorldObjectKind> =
        scene.kinds.iter().map(|k| (k.id.as_str(), k)).collect();
    let mut by_kind: HashMap<&str, Vec<ObjectInstance>> = HashMap::new();
    for inst in &scene.objects.instances {
        // ponytail: `__primitive_` mugshot blocks aren't rendered yet.
        if kinds.contains_key(inst.kind_id.as_str()) {
            by_kind.entry(&inst.kind_id).or_default().push(inst.clone());
        }
    }
    pending.0 = by_kind
        .into_iter()
        .map(|(id, instances)| {
            let kind = kinds[id].clone();
            let gltf = assets.load(base.asset_path(&kind.gltf_path));
            (kind, gltf, instances)
        })
        .collect();
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_loaded_kinds(
    mut commands: Commands,
    mut pending: ResMut<PendingKinds>,
    scene: Res<WorldSceneData>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    gltf_materials: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
) {
    pending.0.retain(|(kind, handle, instances)| {
        if assets.load_state(handle).is_failed() {
            // A kind whose model fails to load is left out.
            return false;
        }
        if !assets.is_loaded_with_dependencies(handle) {
            return true;
        }
        let Some((node_transform, primitive)) = gltfs
            .get(handle)
            .and_then(|g| first_mesh(g, &nodes, &gltf_meshes))
        else {
            warn!("no mesh in {}", kind.gltf_path);
            return false;
        };
        let base = primitive
            .material
            .as_ref()
            .and_then(|m| gltf_materials.get(m))
            .map(standard_material_from_gltf_material)
            .unwrap_or_default();
        let material = materials.add(ObjectMaterial {
            base: apply_overrides(base, kind),
            extension: HemisphereFill::new(&scene.lighting),
        });
        for inst in instances {
            commands.spawn((
                SceneEntity,
                Mesh3d(primitive.mesh.clone()),
                MeshMaterial3d(material.clone()),
                mesh_transform(inst, kind).mul_transform(node_transform),
            ));
        }
        false
    });
}

/// The first mesh primitive in scene-traversal order, with its node's
/// accumulated transform (quantized models keep their dequantization on
/// the node).
fn first_mesh(
    gltf: &Gltf,
    nodes: &Assets<GltfNode>,
    meshes: &Assets<GltfMesh>,
) -> Option<(Transform, bevy::gltf::GltfPrimitive)> {
    let child_ids: std::collections::HashSet<_> = gltf
        .nodes
        .iter()
        .filter_map(|h| nodes.get(h))
        .flat_map(|n| n.children.iter().map(Handle::id))
        .collect();
    // ponytail: roots in node-index order, which matches three.js's traversal for single-scene files.
    let mut stack: Vec<(Transform, &Handle<GltfNode>)> = gltf
        .nodes
        .iter()
        .rev()
        .filter(|h| !child_ids.contains(&h.id()))
        .map(|h| (Transform::IDENTITY, h))
        .collect();
    while let Some((parent, handle)) = stack.pop() {
        let node = nodes.get(handle)?;
        let world = parent.mul_transform(node.transform);
        if let Some(primitive) = node
            .mesh
            .as_ref()
            .and_then(|m| meshes.get(m))
            .and_then(|m| m.primitives.first())
        {
            return Some((world, primitive.clone()));
        }
        stack.extend(node.children.iter().rev().map(|c| (world, c)));
    }
    None
}

/// `buildMaterialForKind`: the kind's non-null overrides on top of the
/// glTF material.
pub fn apply_overrides(mut m: StandardMaterial, kind: &WorldObjectKind) -> StandardMaterial {
    if let Some(tint) = kind.tint {
        m.base_color = tint.with_alpha(m.base_color.alpha());
    }
    if kind.opacity < 1.0 {
        // ponytail: a glTF `Mask` cutoff is lost; three.js keeps alphaTest on its transparent clone.
        m.alpha_mode = AlphaMode::Blend;
        m.base_color.set_alpha(kind.opacity);
    }
    if let Some(r) = kind.roughness {
        m.perceptual_roughness = r;
    }
    if let Some(metal) = kind.metalness {
        m.metallic = metal;
    }
    if let Some(e) = kind.emissive {
        m.emissive = (e.to_linear() * kind.emissive_intensity).with_alpha(1.0);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind() -> WorldObjectKind {
        serde_json::from_str(
            r##"{"id":"k","gltfPath":"/k.gltf","scale":1,"tint":"#ff0000","opacity":0.5,
            "roughness":0.2,"metalness":null,"emissive":"#00ff00","emissiveIntensity":2}"##,
        )
        .unwrap()
    }

    #[test]
    fn overrides_replace_only_non_null_fields() {
        let base = StandardMaterial {
            metallic: 0.7,
            ..default()
        };
        let m = apply_overrides(base, &kind());
        assert_eq!(m.base_color, Color::srgba(1.0, 0.0, 0.0, 0.5));
        assert!(matches!(m.alpha_mode, AlphaMode::Blend));
        assert_eq!(m.perceptual_roughness, 0.2);
        assert_eq!(m.metallic, 0.7);
        assert_eq!(m.emissive, LinearRgba::rgb(0.0, 2.0, 0.0));
    }

    #[test]
    fn bad_colours_are_rejected() {
        let bad = r##"{"id":"k","gltfPath":"/k.gltf","scale":1,"tint":"red","opacity":1,
            "roughness":null,"metalness":null,"emissive":null,"emissiveIntensity":0}"##;
        assert!(serde_json::from_str::<WorldObjectKind>(bad).is_err());
    }
}
