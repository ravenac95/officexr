//! Port of `BakedLayout.tsx`: each placement draws every mesh of its
//! layout's GLB.

use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::prelude::*;

use crate::lighting::{HemisphereFill, ObjectMaterial};
use crate::objects::{mesh_primitives, standard_material};
use crate::placement::baked_layout_transform;
use crate::scene::{BakedLayoutPlacement, WorldSceneData};
use crate::{AssetBase, SceneEntity};

#[derive(Resource, Default)]
pub(crate) struct PendingLayouts(Vec<(BakedLayoutPlacement, Handle<Gltf>)>);

pub(crate) fn request_layouts(
    scene: Res<WorldSceneData>,
    base: Res<AssetBase>,
    assets: Res<AssetServer>,
    mut pending: ResMut<PendingLayouts>,
) {
    pending.0 = scene
        .baked_layouts
        .iter()
        .map(|p| (p.clone(), assets.load(base.asset_path(&p.gltf_path))))
        .collect();
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_loaded_layouts(
    mut commands: Commands,
    mut pending: ResMut<PendingLayouts>,
    scene: Res<WorldSceneData>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    gltf_materials: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
) {
    pending.0.retain(|(placement, handle)| {
        if assets.load_state(handle).is_failed() {
            // As in three.js, a layout that isn't baked yet is left out.
            return false;
        }
        if !assets.is_loaded_with_dependencies(handle) {
            return true;
        }
        let Some(gltf) = gltfs.get(handle) else {
            return false;
        };
        let root = baked_layout_transform(placement);
        for (node_transform, primitive) in mesh_primitives(gltf, &nodes, &gltf_meshes) {
            let material = materials.add(ObjectMaterial {
                base: standard_material(&primitive, &gltf_materials),
                extension: HemisphereFill::new(&scene.lighting),
            });
            commands.spawn((
                SceneEntity,
                Mesh3d(primitive.mesh),
                MeshMaterial3d(material),
                root.mul_transform(node_transform),
            ));
        }
        false
    });
}
