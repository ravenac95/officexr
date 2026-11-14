//! Port of `BakedLayout.tsx`: each placement draws every mesh of its
//! layout's GLB.

use bevy::gltf::{Gltf, GltfMaterial, GltfMesh, GltfNode};
use bevy::prelude::*;

use crate::AssetBase;
use crate::lighting::{HemisphereFill, ObjectMaterial};
use crate::objects::{mesh_primitives, standard_material};
use crate::placement::baked_layout_transform;
use crate::scene::{BakedLayoutPlacement, WorldSceneData};

enum LayoutState {
    Loading(Handle<Gltf>),
    Spawned(Vec<Entity>),
    /// As in three.js, a layout that isn't baked yet is left out.
    Failed,
}

struct Layout {
    placement: BakedLayoutPlacement,
    state: LayoutState,
}

#[derive(Resource, Default)]
pub struct LayoutIndex(Vec<Layout>);

impl LayoutIndex {
    pub fn is_loading(&self) -> bool {
        self.0
            .iter()
            .any(|l| matches!(l.state, LayoutState::Loading(_)))
    }
}

/// Keeps the layouts the scene still lists as they are, requests the new,
/// and despawns the gone.
pub(crate) fn sync_layouts(
    mut commands: Commands,
    scene: Res<WorldSceneData>,
    base: Res<AssetBase>,
    assets: Res<AssetServer>,
    mut index: ResMut<LayoutIndex>,
) {
    let mut gone = std::mem::take(&mut index.0);
    index.0 = scene
        .baked_layouts
        .iter()
        .map(|p| match gone.iter().position(|l| l.placement == *p) {
            Some(i) => gone.swap_remove(i),
            None => Layout {
                placement: p.clone(),
                state: LayoutState::Loading(assets.load(base.asset_path(&p.gltf_path))),
            },
        })
        .collect();
    for layout in gone {
        if let LayoutState::Spawned(entities) = layout.state {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_loaded_layouts(
    mut commands: Commands,
    mut index: ResMut<LayoutIndex>,
    scene: Res<WorldSceneData>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    gltf_materials: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
) {
    for layout in &mut index.0 {
        let LayoutState::Loading(handle) = &layout.state else {
            continue;
        };
        if assets.load_state(handle).is_failed() {
            layout.state = LayoutState::Failed;
            continue;
        }
        if !assets.is_loaded_with_dependencies(handle) {
            continue;
        }
        let Some(gltf) = gltfs.get(handle) else {
            layout.state = LayoutState::Failed;
            continue;
        };
        let root = baked_layout_transform(&layout.placement);
        let entities = mesh_primitives(gltf, &nodes, &gltf_meshes)
            .into_iter()
            .map(|(node_transform, primitive)| {
                let material = materials.add(ObjectMaterial {
                    base: standard_material(&primitive, &gltf_materials),
                    extension: HemisphereFill::new(&scene.lighting),
                });
                commands
                    .spawn((
                        Mesh3d(primitive.mesh),
                        MeshMaterial3d(material),
                        root.mul_transform(node_transform),
                    ))
                    .id()
            })
            .collect();
        layout.state = LayoutState::Spawned(entities);
    }
}
