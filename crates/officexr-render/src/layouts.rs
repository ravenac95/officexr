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
    Loading,
    /// Each mesh entity with its transform inside the GLB, to re-pose when the placement moves.
    Spawned(Vec<(Entity, Transform)>),
    /// As in three.js, a layout that isn't baked yet is left out.
    Failed,
}

struct Layout {
    placement: BakedLayoutPlacement,
    /// Held so the GLB stays loaded while the layout is moved.
    gltf: Handle<Gltf>,
    state: LayoutState,
}

#[derive(Resource, Default)]
pub struct LayoutIndex(Vec<Layout>);

impl LayoutIndex {
    pub fn is_loading(&self) -> bool {
        self.0
            .iter()
            .any(|l| matches!(l.state, LayoutState::Loading))
    }

    /// The GLB paths of the layouts that failed to load.
    pub fn failed(&self) -> impl Iterator<Item = &str> {
        self.0
            .iter()
            .filter(|l| matches!(l.state, LayoutState::Failed))
            .map(|l| l.placement.gltf_path.as_str())
    }
}

/// Keeps the layouts the scene still lists as they are, re-poses the moved
/// ones in place, requests the new, and despawns the gone.
pub(crate) fn sync_layouts(
    mut commands: Commands,
    scene: Res<WorldSceneData>,
    base: Res<AssetBase>,
    assets: Res<AssetServer>,
    mut index: ResMut<LayoutIndex>,
) {
    let mut gone = std::mem::take(&mut index.0);
    let exact: Vec<Option<Layout>> = scene
        .baked_layouts
        .iter()
        .map(|p| {
            let i = gone.iter().position(|l| l.placement == *p)?;
            Some(gone.swap_remove(i))
        })
        .collect();
    index.0 = scene
        .baked_layouts
        .iter()
        .zip(exact)
        .map(|(p, kept)| {
            if let Some(layout) = kept {
                return layout;
            }
            let Some(i) = gone
                .iter()
                .position(|l| l.placement.gltf_path == p.gltf_path)
            else {
                return Layout {
                    placement: p.clone(),
                    gltf: assets.load(base.asset_path(&p.gltf_path)),
                    state: LayoutState::Loading,
                };
            };
            let mut layout = gone.swap_remove(i);
            if let LayoutState::Spawned(entities) = &layout.state {
                let root = baked_layout_transform(p);
                for (entity, local) in entities {
                    commands.entity(*entity).insert(root.mul_transform(*local));
                }
            }
            layout.placement = p.clone();
            layout
        })
        .collect();
    for layout in gone {
        if let LayoutState::Spawned(entities) = layout.state {
            for (entity, _) in entities {
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
        if !matches!(layout.state, LayoutState::Loading) {
            continue;
        }
        let handle = &layout.gltf;
        if assets.load_state(handle).is_failed()
            || assets.recursive_dependency_load_state(handle).is_failed()
        {
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
                let entity = commands
                    .spawn((
                        Mesh3d(primitive.mesh),
                        MeshMaterial3d(material),
                        root.mul_transform(node_transform),
                    ))
                    .id();
                (entity, node_transform)
            })
            .collect();
        layout.state = LayoutState::Spawned(entities);
    }
}
