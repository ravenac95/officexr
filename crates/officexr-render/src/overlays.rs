//! The studio editors' overlays, drawn over a view's world: ports of
//! `selectionOutline.ts`, `GhostLayer.tsx`, `EndlessGrid.tsx`,
//! `DirectionGizmo.tsx`, and the map editor's spawn markers, zones and labels.
//!
//! A view's overlays are replaced as a list; each is diffed by its `tag`,
//! so only the added, removed and changed ones are respawned.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use serde::Deserialize;

use crate::lighting::ObjectMaterial;
use crate::objects::Placed;
use crate::picking::PickTarget;
use crate::scene::{ObjectInstance, hex};
use crate::world_label;

const GRID_HALF_SIZE: f32 = 200.0;
const GRID_SECTION_CELLS: f32 = 10.0;
const GRID_CELL_COLOR: Srgba = Srgba::rgb(
    0x3f as f32 / 255.0,
    0x3f as f32 / 255.0,
    0x46 as f32 / 255.0,
);
const GRID_SECTION_COLOR: Srgba = Srgba::rgb(
    0x71 as f32 / 255.0,
    0x71 as f32 / 255.0,
    0x7a as f32 / 255.0,
);
const GIZMO_LENGTH: f32 = 1.5;
const SPAWN_CONE_HEIGHT: f32 = 1.6;
const SPAWN_CONE_RADIUS: f32 = 0.6;
const SPAWN_TIP_RADIUS: f32 = 0.18;
const LABEL_FONT_SIZE: f32 = 12.0;
/// `GhostLayer.tsx`'s pulse: ±50% of the opacity at 4 rad/s.
const PULSE_RATE: f32 = 4.0;

#[derive(Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Overlay {
    pub tag: String,
    /// Off, picks pass through it to what lies behind.
    #[serde(default = "pickable")]
    pub pickable: bool,
    #[serde(flatten)]
    pub kind: OverlayKind,
}

fn pickable() -> bool {
    true
}

/// Colours are `#rrggbb` or, translucent, `#rrggbbaa`.
#[derive(Deserialize, Clone, PartialEq, Debug)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum OverlayKind {
    /// Drawn through other geometry. Edges shared by flush boxes are drawn once.
    Outline {
        #[serde(default)]
        boxes: Vec<WorldAabb>,
        /// World-space line segments.
        #[serde(default)]
        segments: Vec<[[f32; 3]; 2]>,
        #[serde(deserialize_with = "hex")]
        color: Color,
    },
    /// The objects drawn translucent, with the view's kinds.
    Ghost {
        objects: Vec<ObjectInstance>,
        opacity: f32,
        #[serde(default)]
        pulse: bool,
        /// Replaces the kinds' colour, as for a blocked move.
        #[serde(default, deserialize_with = "crate::scene::opt_hex")]
        tint: Option<Color>,
    },
    Grid {
        y: f32,
        cell_size: f32,
        /// The distance from the origin past which the lines have faded out.
        fade: f32,
    },
    /// An arrow from `at` along `direction`, drawn through other geometry.
    DirectionGizmo {
        at: [f32; 3],
        direction: [f32; 3],
        #[serde(deserialize_with = "hex")]
        color: Color,
    },
    Marker {
        shape: MarkerShape,
        pos: [f32; 3],
        yaw: f32,
        #[serde(deserialize_with = "hex")]
        color: Color,
    },
    /// A flat area at height `y`, drawn through other geometry.
    Zone {
        shape: ZoneShape,
        y: f32,
        #[serde(deserialize_with = "hex")]
        color: Color,
    },
    Label {
        at: [f32; 3],
        text: String,
    },
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct WorldAabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum MarkerShape {
    /// A cone standing on `pos`, as the map editor marks spawn points.
    SpawnCone,
}

/// In the XZ plane.
#[derive(Deserialize, Clone, PartialEq, Debug)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ZoneShape {
    // ponytail: filled as a fan from the first point, so only convex polygons fill right.
    Polygon { points: Vec<[f32; 2]> },
    Circle { center: [f32; 2], radius: f32 },
}

/// The overlays a view should draw.
#[derive(Component, Deref, Default)]
pub struct ViewOverlays(pub Vec<Overlay>);

impl ViewOverlays {
    pub(crate) fn ghost_objects(&self) -> impl Iterator<Item = &ObjectInstance> {
        self.iter().flat_map(|o| match &o.kind {
            OverlayKind::Ghost { objects, .. } => objects.as_slice(),
            _ => &[],
        })
    }
}

/// The entity drawing each of a view's overlays, by tag, with what it draws.
#[derive(Component, Default)]
pub struct OverlayIndex(HashMap<String, (Entity, Overlay)>);

/// An unlit colour, drawn over everything already drawn, as three.js's
/// `depthTest: false` overlays.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct OverlayMaterial {
    #[uniform(0)]
    color: LinearRgba,
}

impl Material for OverlayMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://officexr_render/overlay.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_compare = Some(CompareFunction::Always);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct GridMaterial {
    #[uniform(0)]
    grid: GridUniform,
}

#[derive(ShaderType, Debug, Clone)]
struct GridUniform {
    cell_color: LinearRgba,
    section_color: LinearRgba,
    cell_size: f32,
    section_size: f32,
    fade_distance: f32,
}

impl Material for GridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://officexr_render/grid.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// A ghost overlay's look, and its translucent copy of each kind material
/// its objects were drawn with.
#[derive(Component)]
pub(crate) struct Ghost {
    opacity: f32,
    pulse: bool,
    tint: Option<Color>,
    // ponytail: copies miss later lighting edits until the overlay changes; fine while ghosts are transient.
    materials: HashMap<AssetId<ObjectMaterial>, Handle<ObjectMaterial>>,
}

impl Ghost {
    /// `GhostLayer.tsx`'s material override.
    fn dress(&self, kind: &ObjectMaterial) -> ObjectMaterial {
        let mut ghost = kind.clone();
        let base = &mut ghost.base;
        if let Some(tint) = self.tint {
            base.base_color = tint;
        }
        base.base_color.set_alpha(self.opacity);
        base.alpha_mode = AlphaMode::Blend;
        base.double_sided = true;
        base.cull_mode = None;
        ghost
    }
}

#[derive(SystemParam)]
pub(crate) struct OverlayAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    overlays: ResMut<'w, Assets<OverlayMaterial>>,
    grids: ResMut<'w, Assets<GridMaterial>>,
    standard: ResMut<'w, Assets<StandardMaterial>>,
}

/// Respawns each overlay whose tag is new or whose content changed, and
/// despawns those gone.
pub(crate) fn sync_overlays(
    mut commands: Commands,
    mut views: Query<(Entity, &ViewOverlays, &mut OverlayIndex), Changed<ViewOverlays>>,
    mut assets: OverlayAssets,
) {
    for (view, overlays, mut index) in &mut views {
        let mut wanted: HashMap<&str, &Overlay> = HashMap::new();
        for overlay in overlays.iter() {
            if wanted.insert(overlay.tag.as_str(), overlay).is_some() {
                warn!("duplicate overlay tag {}", overlay.tag);
            }
        }
        index.0.retain(|tag, (entity, drawn)| {
            let keep = wanted.get(tag.as_str()) == Some(&&*drawn);
            if !keep {
                commands.entity(*entity).despawn();
            }
            keep
        });
        for (tag, overlay) in wanted {
            if !index.0.contains_key(tag) {
                let entity = spawn(&mut commands, view, overlay, &mut assets);
                index.0.insert(tag.to_string(), (entity, overlay.clone()));
            }
        }
    }
}

fn spawn(
    commands: &mut Commands,
    view: Entity,
    overlay: &Overlay,
    assets: &mut OverlayAssets,
) -> Entity {
    let root = commands
        .spawn((
            PickTarget::Overlay {
                tag: overlay.tag.clone(),
            },
            Transform::default(),
            Visibility::default(),
            ChildOf(view),
        ))
        .id();
    #[cfg(feature = "picking")]
    if !overlay.pickable {
        commands
            .entity(root)
            .insert(bevy::app::Propagate(bevy::picking::Pickable::IGNORE));
    }
    let x_ray = |assets: &mut OverlayAssets, mesh: Mesh, color: Color, transform: Transform| {
        (
            Mesh3d(assets.meshes.add(mesh)),
            MeshMaterial3d(assets.overlays.add(OverlayMaterial {
                color: color.to_linear(),
            })),
            transform,
            ChildOf(root),
        )
    };
    match &overlay.kind {
        OverlayKind::Outline {
            boxes,
            segments,
            color,
        } => {
            let mut lines = box_edges(boxes);
            lines.extend(segments.iter().flatten().map(|&p| Vec3::from(p)));
            let mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, lines);
            commands.spawn(x_ray(assets, mesh, *color, Transform::default()));
        }
        OverlayKind::Ghost {
            objects,
            opacity,
            pulse,
            tint,
        } => {
            commands.entity(root).insert(Ghost {
                opacity: *opacity,
                pulse: *pulse,
                tint: *tint,
                materials: HashMap::new(),
            });
            // `attach_models` draws them with their kind's model, as it draws objects.
            for object in objects {
                commands.spawn((Placed(object.clone()), NotShadowCaster, ChildOf(root)));
            }
        }
        OverlayKind::Grid { y, cell_size, fade } => {
            let grid = GridMaterial {
                grid: GridUniform {
                    cell_color: GRID_CELL_COLOR.into(),
                    section_color: GRID_SECTION_COLOR.into(),
                    cell_size: *cell_size,
                    section_size: cell_size * GRID_SECTION_CELLS,
                    fade_distance: *fade,
                },
            };
            let plane = Plane3d::default()
                .mesh()
                .size(GRID_HALF_SIZE * 2.0, GRID_HALF_SIZE * 2.0);
            commands.spawn((
                Mesh3d(assets.meshes.add(plane)),
                MeshMaterial3d(assets.grids.add(grid)),
                Transform::from_xyz(0.0, *y, 0.0),
                ChildOf(root),
            ));
        }
        OverlayKind::DirectionGizmo {
            at,
            direction,
            color,
        } => {
            let shaft = GIZMO_LENGTH * 0.7;
            let head = GIZMO_LENGTH * 0.3;
            let direction = Vec3::from(*direction).try_normalize().unwrap_or(Vec3::Y);
            commands.entity(root).insert(
                Transform::from_translation((*at).into())
                    .with_rotation(Quat::from_rotation_arc(Vec3::Y, direction)),
            );
            let shaft_mesh = Cylinder::new(0.05, shaft).mesh().resolution(8).build();
            let head_mesh = Cone {
                radius: 0.15,
                height: head,
            }
            .mesh()
            .resolution(8)
            .build();
            commands.spawn(x_ray(
                assets,
                shaft_mesh,
                *color,
                Transform::from_xyz(0.0, shaft / 2.0, 0.0),
            ));
            commands.spawn(x_ray(
                assets,
                head_mesh,
                *color,
                Transform::from_xyz(0.0, shaft + head / 2.0, 0.0),
            ));
        }
        OverlayKind::Marker {
            shape: MarkerShape::SpawnCone,
            pos,
            yaw,
            color,
        } => {
            commands.entity(root).insert(
                Transform::from_translation((*pos).into())
                    .with_rotation(Quat::from_rotation_y(*yaw)),
            );
            let cone = Cone {
                radius: SPAWN_CONE_RADIUS,
                height: SPAWN_CONE_HEIGHT,
            }
            .mesh()
            .resolution(8);
            let cone_material = StandardMaterial {
                base_color: *color,
                emissive: color.to_linear() * 0.6,
                ..default()
            };
            let tip_material = StandardMaterial {
                base_color: color.mix(&Color::WHITE, 0.75),
                unlit: true,
                ..default()
            };
            commands.spawn((
                Mesh3d(assets.meshes.add(cone)),
                MeshMaterial3d(assets.standard.add(cone_material)),
                Transform::from_xyz(0.0, SPAWN_CONE_HEIGHT / 2.0, 0.0),
                NotShadowCaster,
                ChildOf(root),
            ));
            commands.spawn((
                Mesh3d(
                    assets
                        .meshes
                        .add(Sphere::new(SPAWN_TIP_RADIUS).mesh().uv(8, 8)),
                ),
                MeshMaterial3d(assets.standard.add(tip_material)),
                Transform::from_xyz(0.0, SPAWN_CONE_HEIGHT + 0.1, 0.0),
                NotShadowCaster,
                ChildOf(root),
            ));
        }
        OverlayKind::Zone { shape, y, color } => {
            let (mesh, transform) = match shape {
                ZoneShape::Polygon { points } => (fan(points, *y), Transform::default()),
                ZoneShape::Circle { center, radius } => (
                    Circle::new(*radius).mesh().resolution(48).build(),
                    Transform::from_xyz(center[0], *y, center[1])
                        .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
                ),
            };
            commands.spawn(x_ray(assets, mesh, *color, transform));
        }
        OverlayKind::Label { at, text } => {
            commands
                .entity(root)
                .insert(Transform::from_translation((*at).into()));
            commands.spawn(world_label::label(
                text.clone(),
                LABEL_FONT_SIZE,
                Color::NONE,
                root,
                Vec3::ZERO,
            ));
        }
    }
    root
}

/// The line-list positions of the boxes' edges, each edge once, so flush
/// boxes outline as one.
fn box_edges(boxes: &[WorldAabb]) -> Vec<Vec3> {
    let mut seen = HashSet::new();
    let mut lines = vec![];
    for b in boxes {
        let corner = |i: usize| {
            Vec3::new(
                if i & 1 == 0 { b.min[0] } else { b.max[0] },
                if i & 2 == 0 { b.min[1] } else { b.max[1] },
                if i & 4 == 0 { b.min[2] } else { b.max[2] },
            )
        };
        // Corner pairs differing in one axis bit: x, then y, then z edges.
        for axis in [1, 2, 4] {
            for from in (0..8).filter(|i| i & axis == 0) {
                let (a, b) = (corner(from), corner(from | axis));
                let key = (
                    a.to_array().map(f32::to_bits),
                    b.to_array().map(f32::to_bits),
                );
                if seen.insert(key) {
                    lines.extend([a, b]);
                }
            }
        }
    }
    lines
}

/// The polygon at height `y`, facing up whichever way it winds.
fn fan(points: &[[f32; 2]], y: f32) -> Mesh {
    let area: f32 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum();
    let n = points.len() as u32;
    let indices = (1..n.saturating_sub(1)).flat_map(|i| {
        // A positive XZ area winds clockwise seen from above.
        if area > 0.0 {
            [0, i + 1, i]
        } else {
            [0, i, i + 1]
        }
    });
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        points
            .iter()
            .map(|p| Vec3::new(p[0], y, p[1]))
            .collect::<Vec<_>>(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![Vec3::Y; points.len()])
    .with_inserted_indices(Indices::U32(indices.collect()))
}

/// Swaps each newly drawn ghost object's kind material for its ghost's translucent copy.
pub(crate) fn dress_ghosts(
    mut drawn: Query<
        (&mut MeshMaterial3d<ObjectMaterial>, &ChildOf),
        (With<Placed>, Added<Mesh3d>),
    >,
    mut ghosts: Query<&mut Ghost>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
) {
    for (mut material, root) in &mut drawn {
        let Ok(mut ghost) = ghosts.get_mut(root.parent()) else {
            continue;
        };
        let kind = material.id();
        if !ghost.materials.contains_key(&kind) {
            let Some(dressed) = materials.get(kind).map(|m| ghost.dress(m)) else {
                continue;
            };
            ghost.materials.insert(kind, materials.add(dressed));
        }
        material.0 = ghost.materials[&kind].clone();
    }
}

pub(crate) fn pulse_ghosts(
    time: Res<Time>,
    ghosts: Query<&Ghost>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
) {
    let wave = 1.0 + 0.5 * (time.elapsed_secs() * PULSE_RATE).sin();
    for ghost in ghosts.iter().filter(|g| g.pulse) {
        for handle in ghost.materials.values() {
            if let Some(mut material) = materials.get_mut(handle) {
                material.base.base_color.set_alpha(ghost.opacity * wave);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::camera::visibility::RenderLayers;

    use super::*;
    use crate::WorldDiff;
    use crate::tests::{bare_app, camera, scene};
    use crate::views::{ViewHub, ViewId, ViewKind, ViewMsg, ViewTarget, find_view};

    /// A view of one blue cube.
    fn view(app: &mut App) -> (ViewId, Entity) {
        let hub = app.world().resource::<ViewHub>().clone();
        let id = hub.create(ViewKind::World, |_| ViewTarget::Image(UVec2::new(8, 8)));
        let mut diff: WorldDiff =
            serde_json::from_str(include_str!("../examples/booper.scene.json")).unwrap();
        diff.kinds = Some(vec![]);
        diff.objects = scene(&[("a", [0.0; 3])]).objects.into();
        hub.send(id, ViewMsg::World(Box::new(diff)));
        hub.send(id, ViewMsg::Camera(camera(0.0)));
        app.update();
        (id, find_view(app.world_mut(), id).unwrap())
    }

    fn set(app: &mut App, id: ViewId, overlays: serde_json::Value) {
        let overlays = serde_json::from_value(overlays).unwrap();
        let hub = app.world().resource::<ViewHub>().clone();
        assert!(hub.send(id, ViewMsg::Overlays(overlays)));
        app.update();
        app.update();
    }

    fn drawn(app: &mut App, view: Entity) -> HashMap<String, Entity> {
        let index = app.world().get::<OverlayIndex>(view).unwrap();
        index
            .0
            .iter()
            .map(|(tag, (e, _))| (tag.clone(), *e))
            .collect()
    }

    fn outline(color: &str) -> serde_json::Value {
        serde_json::json!({"tag": "sel", "kind": "outline", "segments": [[[0, 0, 0], [1, 0, 0]]], "color": color})
    }

    #[test]
    fn only_changed_overlays_are_respawned() {
        let mut app = bare_app();
        let (id, view) = view(&mut app);
        let zone = serde_json::json!({"tag": "zone", "kind": "zone",
            "shape": {"type": "polygon", "points": [[0, 0], [1, 0], [1, 1]]}, "y": 0.05, "color": "#ff00ff66"});
        set(&mut app, id, serde_json::json!([outline("#fde68a"), zone]));
        let before = drawn(&mut app, view);
        assert_eq!(before.len(), 2);
        let entities = app.world().iter_entities().count();

        set(&mut app, id, serde_json::json!([outline("#ef4444"), zone]));
        let after = drawn(&mut app, view);
        assert_eq!(after["zone"], before["zone"]);
        assert_ne!(after["sel"], before["sel"]);
        assert!(app.world().get_entity(before["sel"]).is_err());
        assert_eq!(app.world().iter_entities().count(), entities);

        set(&mut app, id, serde_json::json!([zone]));
        assert_eq!(app.world().iter_entities().count(), entities - 2);
    }

    #[test]
    fn an_overlay_is_a_pick_target_drawn_only_in_its_view() {
        let mut app = bare_app();
        let (id, view) = view(&mut app);
        let (_, other) = self::view(&mut app);
        set(&mut app, id, serde_json::json!([outline("#fde68a")]));
        let root = drawn(&mut app, view)["sel"];
        let tag = "sel".to_string();
        assert_eq!(
            app.world().get::<PickTarget>(root),
            Some(&PickTarget::Overlay { tag })
        );
        let line = app.world().get::<Children>(root).unwrap()[0];
        let layers = |e: Entity| app.world().get::<RenderLayers>(e).cloned();
        assert_eq!(layers(line), layers(view));
        assert_ne!(layers(line), layers(other));
    }

    #[test]
    fn ghosts_draw_their_kind_translucent_even_when_no_object_uses_it() {
        let mut app = bare_app();
        let (id, view) = view(&mut app);
        set(
            &mut app,
            id,
            serde_json::json!([{"tag": "add", "kind": "ghost", "opacity": 0.5,
            "objects": [{"id": "g", "kindId": "__primitive_stone", "position": [4, 0, 0]}]}]),
        );
        let root = drawn(&mut app, view)["add"];
        let ghost = app.world().get::<Children>(root).unwrap()[0];
        let handle = app
            .world()
            .get::<MeshMaterial3d<ObjectMaterial>>(ghost)
            .unwrap();
        let material = &app
            .world()
            .resource::<Assets<ObjectMaterial>>()
            .get(handle)
            .unwrap()
            .base;
        assert_eq!(material.alpha_mode, AlphaMode::Blend);
        assert_eq!(material.base_color.alpha(), 0.5);
        assert!(app.world().get::<Placed>(ghost).is_some());
        assert!(
            app.world().get::<PickTarget>(ghost).is_none(),
            "picks bubble to the overlay"
        );
    }

    fn aabb(min: [f32; 3], max: [f32; 3]) -> WorldAabb {
        WorldAabb { min, max }
    }

    #[test]
    fn flush_boxes_share_their_touching_rim() {
        assert_eq!(box_edges(&[aabb([0.0; 3], [1.0; 3])]).len(), 12 * 2);
        let flush = [
            aabb([0.0; 3], [1.0; 3]),
            aabb([1.0, 0.0, 0.0], [2.0, 1.0, 1.0]),
        ];
        // The second box's 4 edges on x = 1 are the first's.
        assert_eq!(box_edges(&flush).len(), (12 + 8) * 2);
    }

    #[test]
    fn a_polygon_faces_up_whichever_way_it_winds() {
        let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let reversed: Vec<_> = square.iter().rev().copied().collect();
        for points in [&square[..], &reversed] {
            let mesh = fan(points, 0.5);
            let Some(Indices::U32(indices)) = mesh.indices() else {
                panic!("no indices");
            };
            assert_eq!(indices.len(), 6);
            let p = |i: u32| {
                let [x, z] = points[i as usize];
                Vec3::new(x, 0.5, z)
            };
            for tri in indices.chunks(3) {
                let normal = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
                assert!(normal.y > 0.0, "{points:?}: {tri:?} faces down");
            }
        }
    }

    #[test]
    fn overlays_read_the_ts_shapes() {
        let overlays: Vec<Overlay> = serde_json::from_value(serde_json::json!([
            {"tag": "sel", "kind": "outline", "boxes": [{"min": [0, 0, 0], "max": [1, 1, 1]}], "color": "#fde68af2"},
            {"tag": "g", "kind": "ghost", "pickable": false, "objects": [{"id": "a", "kindId": "k", "position": [0, 0, 0]}], "opacity": 0.5, "pulse": true},
            {"tag": "zone", "kind": "zone", "shape": {"type": "circle", "center": [1, 2], "radius": 3}, "y": 0.05, "color": "#ff00ff66"},
            {"tag": "spawn", "kind": "marker", "shape": "spawnCone", "pos": [0, 0, 0], "yaw": 0, "color": "#22d3ee"},
        ]))
        .unwrap();
        assert!(overlays[0].pickable);
        assert!(!overlays[1].pickable);
        assert!(matches!(
            overlays[1].kind,
            OverlayKind::Ghost { pulse: true, .. }
        ));
        let OverlayKind::Zone { color, .. } = overlays[2].kind else {
            panic!("not a zone");
        };
        assert!((color.alpha() - 0x66 as f32 / 255.0).abs() < 1e-6);
    }
}
