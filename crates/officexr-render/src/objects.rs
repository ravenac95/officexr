//! Port of `ObjectInstances.tsx` + `cube-material.ts`: every instance of a
//! kind shares one mesh and one material, which Bevy batches into a single
//! instanced draw.

use std::collections::{HashMap, HashSet};

use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::pbr::gltf::standard_material_from_gltf_material;
use bevy::prelude::*;

use crate::AssetBase;
use crate::lighting::{HemisphereFill, ObjectMaterial};
use crate::placement::mesh_transform;
use crate::scene::{LocalAabb, ObjectInstance, WorldObjectKind, WorldSceneData, Xyz};

const PRIMITIVE_KIND_PREFIX: &str = "__primitive_";
const PRIMITIVE_BLOCK_SIZE: f32 = 2.0;

/// The mesh and material one kind's instances share.
#[derive(Clone)]
pub(crate) struct Model {
    mesh: Handle<Mesh>,
    material: Handle<ObjectMaterial>,
    node_transform: Transform,
}

enum ModelState {
    Loading,
    Ready(Model),
    /// A kind whose model fails to load is left out.
    Failed,
}

struct KindModel {
    kind: WorldObjectKind,
    /// `None` for `__primitive_<colour>` kinds, which have no catalog entry.
    gltf: Option<Handle<Gltf>>,
    state: ModelState,
}

#[derive(Resource, Default)]
pub struct ObjectKinds(HashMap<String, KindModel>);

impl ObjectKinds {
    pub fn is_loading(&self) -> bool {
        self.0
            .values()
            .any(|m| matches!(m.state, ModelState::Loading))
    }

    fn placed(&self, inst: &ObjectInstance) -> Option<(&Model, Transform)> {
        let kind = self.0.get(&inst.kind_id)?;
        let ModelState::Ready(model) = &kind.state else {
            return None;
        };
        let transform = mesh_transform(inst, &kind.kind).mul_transform(model.node_transform);
        Some((model, transform))
    }
}

/// The entity drawing each object, by object id.
#[derive(Resource, Default)]
pub struct ObjectIndex(HashMap<String, Entity>);

impl ObjectIndex {
    pub fn get(&self, id: &str) -> Option<Entity> {
        self.0.get(id).copied()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The instance an object entity draws, kept to diff against the next scene.
#[derive(Component)]
pub(crate) struct Placed(ObjectInstance);

#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_kinds(
    mut commands: Commands,
    scene: Res<WorldSceneData>,
    base: Res<AssetBase>,
    assets: Res<AssetServer>,
    mut kinds: ResMut<ObjectKinds>,
    placed: Query<(Entity, &Placed)>,
    mut cuboid: Local<Option<Handle<Mesh>>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
) {
    let wanted: HashSet<&str> = scene.kinds.iter().map(|k| k.id.as_str()).collect();
    kinds
        .0
        .retain(|id, m| m.gltf.is_none() || wanted.contains(id.as_str()));
    let mut redefined = HashSet::new();
    for kind in &scene.kinds {
        if kinds.0.get(&kind.id).is_some_and(|m| m.kind == *kind) {
            continue;
        }
        redefined.insert(kind.id.as_str());
        kinds.0.insert(
            kind.id.clone(),
            KindModel {
                kind: kind.clone(),
                gltf: Some(assets.load(base.asset_path(&kind.gltf_path))),
                state: ModelState::Loading,
            },
        );
    }
    for inst in &scene.objects.instances {
        let Some(colour) = inst.kind_id.strip_prefix(PRIMITIVE_KIND_PREFIX) else {
            continue;
        };
        if kinds.0.contains_key(&inst.kind_id) {
            continue;
        }
        let mesh = cuboid
            .get_or_insert_with(|| meshes.add(Cuboid::from_length(PRIMITIVE_BLOCK_SIZE)))
            .clone();
        let material = materials.add(ObjectMaterial {
            // three.js's `MeshStandardMaterial` defaults.
            base: StandardMaterial {
                base_color: primitive_colour(colour).into(),
                perceptual_roughness: 1.0,
                ..default()
            },
            extension: HemisphereFill::new(&scene.lighting),
        });
        kinds.0.insert(
            inst.kind_id.clone(),
            KindModel {
                kind: primitive_kind(&inst.kind_id),
                gltf: None,
                state: ModelState::Ready(Model {
                    mesh,
                    material,
                    node_transform: Transform::IDENTITY,
                }),
            },
        );
    }
    // The entities keep their place; `attach_models` redraws them once the new model is ready.
    for (entity, placed) in &placed {
        if redefined.contains(placed.0.kind_id.as_str()) {
            commands
                .entity(entity)
                .remove::<(Mesh3d, MeshMaterial3d<ObjectMaterial>)>();
        }
    }
}

/// `PrimitiveInstanceGroup`: the mugshot's `__primitive_<colour>` kinds are
/// plain 2 m boxes the size of their colliders.
fn primitive_kind(id: &str) -> WorldObjectKind {
    let half = PRIMITIVE_BLOCK_SIZE / 2.0;
    let corner = |v: f32| Xyz { x: v, y: v, z: v };
    WorldObjectKind {
        id: id.to_string(),
        gltf_path: String::new(),
        scale: 1.0,
        tint: None,
        opacity: 1.0,
        roughness: None,
        metalness: None,
        emissive: None,
        emissive_intensity: 0.0,
        dimensions: None,
        local_aabb: Some(LocalAabb {
            min: corner(-half),
            max: corner(half),
        }),
    }
}

fn primitive_colour(name: &str) -> Srgba {
    match name {
        "blue" => Srgba::rgb_u8(0x3b, 0x6b, 0xf2),
        "stone" => Srgba::rgb_u8(0xa8, 0xa8, 0xa8),
        _ => Srgba::rgb_u8(0xff, 0x00, 0xff),
    }
}

/// Diffs the scene's instances against the entities by object id: spawns
/// the new, despawns the gone, and re-poses or re-kinds the changed.
/// Instances of an unknown kind are left out.
pub(crate) fn sync_objects(
    mut commands: Commands,
    scene: Res<WorldSceneData>,
    kinds: Res<ObjectKinds>,
    mut index: ResMut<ObjectIndex>,
    mut objects: Query<(&mut Placed, Option<&mut Transform>)>,
) {
    let wanted: HashMap<&str, &ObjectInstance> = scene
        .objects
        .instances
        .iter()
        .filter(|i| kinds.0.contains_key(&i.kind_id))
        .map(|i| (i.id.as_str(), i))
        .collect();
    index.0.retain(|id, entity| {
        let keep = wanted.contains_key(id.as_str());
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    for (id, inst) in wanted {
        let Some(&entity) = index.0.get(id) else {
            let entity = commands.spawn(Placed(inst.clone())).id();
            index.0.insert(id.to_string(), entity);
            continue;
        };
        let Ok((mut placed, transform)) = objects.get_mut(entity) else {
            continue;
        };
        if placed.0 == *inst {
            continue;
        }
        let same_kind = placed.0.kind_id == inst.kind_id;
        placed.0 = inst.clone();
        if !same_kind {
            commands
                .entity(entity)
                .remove::<(Mesh3d, MeshMaterial3d<ObjectMaterial>)>();
        } else if let (Some(mut transform), Some((_, moved))) = (transform, kinds.placed(inst)) {
            *transform = moved;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn load_kinds(
    mut kinds: ResMut<ObjectKinds>,
    scene: Res<WorldSceneData>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    gltf_materials: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
) {
    if !kinds.is_loading() {
        return;
    }
    for model in kinds.0.values_mut() {
        let Some(handle) = model
            .gltf
            .clone()
            .filter(|_| matches!(model.state, ModelState::Loading))
        else {
            continue;
        };
        if assets.load_state(&handle).is_failed() {
            model.state = ModelState::Failed;
            continue;
        }
        if !assets.is_loaded_with_dependencies(&handle) {
            continue;
        }
        let Some((node_transform, primitive)) = gltfs
            .get(&handle)
            .and_then(|g| mesh_primitives(g, &nodes, &gltf_meshes).into_iter().next())
        else {
            warn!("no mesh in {}", model.kind.gltf_path);
            model.state = ModelState::Failed;
            continue;
        };
        let material = materials.add(ObjectMaterial {
            base: apply_overrides(standard_material(&primitive, &gltf_materials), &model.kind),
            extension: HemisphereFill::new(&scene.lighting),
        });
        model.state = ModelState::Ready(Model {
            mesh: primitive.mesh,
            material,
            node_transform,
        });
    }
}

/// Draws every object whose model is ready and which isn't drawn yet.
pub(crate) fn attach_models(
    mut commands: Commands,
    kinds: Res<ObjectKinds>,
    undrawn: Query<(Entity, &Placed), Without<Mesh3d>>,
) {
    for (entity, placed) in &undrawn {
        if let Some((model, transform)) = kinds.placed(&placed.0) {
            commands.entity(entity).insert((
                Mesh3d(model.mesh.clone()),
                MeshMaterial3d(model.material.clone()),
                transform,
            ));
        }
    }
}

/// Every mesh primitive in scene-traversal order, with its node's
/// accumulated transform (quantized models keep their dequantization on
/// the node).
pub(crate) fn mesh_primitives(
    gltf: &Gltf,
    nodes: &Assets<GltfNode>,
    meshes: &Assets<GltfMesh>,
) -> Vec<(Transform, bevy::gltf::GltfPrimitive)> {
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
    let mut out = Vec::new();
    while let Some((parent, handle)) = stack.pop() {
        let Some(node) = nodes.get(handle) else {
            continue;
        };
        let world = parent.mul_transform(node.transform);
        if let Some(mesh) = node.mesh.as_ref().and_then(|m| meshes.get(m)) {
            out.extend(mesh.primitives.iter().map(|p| (world, p.clone())));
        }
        stack.extend(node.children.iter().rev().map(|c| (world, c)));
    }
    out
}

pub(crate) fn standard_material(
    primitive: &bevy::gltf::GltfPrimitive,
    gltf_materials: &Assets<GltfMaterial>,
) -> StandardMaterial {
    primitive
        .material
        .as_ref()
        .and_then(|m| gltf_materials.get(m))
        .map(standard_material_from_gltf_material)
        .unwrap_or_default()
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
