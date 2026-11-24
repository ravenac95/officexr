//! Port of `occlusion/OcclusionController.tsx`, `OccluderRegistry.ts`,
//! `InstancedOccluder.ts` and `MeshOccluder.ts`: a world view's objects and
//! baked-layout meshes that hide the local player fade, and, from above,
//! the roof over the player is cut away.
//!
//! The host sends an [`OcclusionInput`] each frame (`setOcclusion`); every
//! [`FRAMES_PER_QUERY`]th frame the occluders crossing the camera's line of
//! sight to its targets are marked, and every frame each occluder's opacity
//! steps toward its mark. A faded occluder draws with a translucent copy of
//! its material that casts no shadow; a cut one is hidden. Clearing the
//! input restores them all.

pub mod math;

use std::collections::{HashMap, HashSet};

use bevy::light::NotShadowCaster;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use serde::Deserialize;

use crate::lighting::ObjectMaterial;
use crate::objects::Placed;
use crate::picking::PickTarget;
use crate::views::ViewCamera;
use math::*;

/// The line of sight is tested every this many frames; fades step every frame.
pub const FRAMES_PER_QUERY: u32 = 2;
/// A layout mesh with more triangles than this is too costly to ray-test: it only ever cuts away.
pub const MAX_RAYCAST_TRIANGLES: usize = 6000;
/// A layout mesh that can't fade still cuts away when it is all above this.
const MIN_OVERHEAD_BOTTOM: f32 = 1.5;

/// What the camera must keep in sight this frame.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct OcclusionInput {
    /// World points the camera must see: the local player's head and chest.
    pub targets: Vec<[f32; 3]>,
    /// Object ids that never occlude: the seat, ridden rack or pool the player is in.
    pub ignore: Vec<String>,
    /// A point the player occupies (a seat, a ride, a pool): an object holding it never occludes.
    pub anchor: Option<[f32; 3]>,
    /// The local player's feet: set, the cover overhead is cut away around them.
    pub cutaway: Option<[f32; 3]>,
    /// Other players in a seat, ride or pool, to tell whether anything but that hides them.
    pub occupants: Vec<Occupant>,
}

/// A player occupying an object, whose silhouette should show only when
/// something other than that object hides it.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Occupant {
    pub id: String,
    pub targets: Vec<[f32; 3]>,
    pub ignore: Vec<String>,
    pub anchor: Option<[f32; 3]>,
}

/// The view's occlusion input; `None` (or no component) restores every occluder.
#[derive(Component, Default)]
pub struct ViewOcclusion(pub Option<OcclusionInput>);

/// What occlusion did on the view's last frame.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct OcclusionStats {
    /// Occluders drawn faded or cut.
    pub fading: usize,
    /// Occluders cut away over the player.
    pub cut: usize,
    /// The occupants nothing but their own seat, ride or pool hides: their
    /// silhouette should not show.
    pub silhouette_suppressed: Vec<String>,
}

/// A view's marks between queries.
#[derive(Component, Default)]
pub struct OcclusionMarks {
    frame: u32,
    wanted: HashSet<Entity>,
    cut: HashSet<Entity>,
}

/// An occluder drawn faded: its own material, and the translucent copy it draws with.
#[derive(Component)]
pub struct Faded {
    original: Handle<ObjectMaterial>,
    ghost: Handle<ObjectMaterial>,
    pub opacity: f32,
}

/// One occluder's world-space box, from its mesh's local bounds.
#[derive(Clone, Debug)]
pub struct OccluderBox {
    pub world: Mat4,
    pub inverse: Mat4,
    pub min: Vec3,
    pub max: Vec3,
    pub centre: Vec3,
    pub radius: f32,
    /// The lowest corner's world y.
    pub bottom: f32,
    /// Fades in the line of sight; floor-like boxes and large meshes don't.
    pub fadeable: bool,
    /// A roof or ceiling by name, cut away at a lower clearance.
    pub named: bool,
}

impl OccluderBox {
    pub fn new(world: Mat4, min: Vec3, max: Vec3, fadeable: bool, named: bool) -> Self {
        let corners = (0..8).map(|c| {
            world.transform_point3(Vec3::new(
                if c & 1 == 0 { min.x } else { max.x },
                if c & 2 == 0 { min.y } else { max.y },
                if c & 4 == 0 { min.z } else { max.z },
            ))
        });
        let (lo, hi) = corners.fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)));
        let (scale, ..) = world.to_scale_rotation_translation();
        Self {
            world,
            inverse: world.inverse(),
            min,
            max,
            centre: world.transform_point3((min + max) / 2.0),
            radius: (max - min).length() / 2.0 * scale.max_element(),
            bottom: lo.y,
            fadeable: fadeable && !is_floor_like(hi.y - lo.y),
            named,
        }
    }

    /// Where `a → b` (world space) enters the box, as a fraction of it.
    fn entry(&self, a: Vec3, b: Vec3) -> Option<f32> {
        let r = self.radius;
        let c = self.centre;
        if a.x.max(b.x) < c.x - r || a.x.min(b.x) > c.x + r {
            return None;
        }
        if a.z.max(b.z) < c.z - r || a.z.min(b.z) > c.z + r {
            return None;
        }
        segment_box_entry(
            self.inverse.transform_point3(a),
            self.inverse.transform_point3(b),
            self.min,
            self.max,
        )
    }

    /// Whether `point` is the box the player occupies (with a margin); a room-sized box never is.
    pub fn holds(&self, point: Vec3) -> bool {
        if self.radius > ANCHOR_MAX_RADIUS {
            return false;
        }
        let local = self.inverse.transform_point3(point);
        local.cmpge(self.min - ANCHOR_MARGIN).all() && local.cmple(self.max + ANCHOR_MARGIN).all()
    }

    /// Whether the vertical ray from the head of feet at `feet` hits the box overhead.
    fn covers(&self, feet: Vec3) -> bool {
        if !is_overhead_cover(self.bottom, feet.y, self.named) {
            return false;
        }
        if (feet.x - self.centre.x).abs() > self.radius
            || (feet.z - self.centre.z).abs() > self.radius
        {
            return false;
        }
        let head = feet + Vec3::Y;
        self.entry(head, head + Vec3::Y * ROOF_REACH).is_some()
    }
}

/// An occluder: a box, and for a layout mesh, its triangles to confirm a box hit.
pub struct Occluder<'a> {
    pub entity: Entity,
    pub bounds: OccluderBox,
    /// An object's id, which `ignore` lists; `None` for a layout mesh.
    pub id: Option<&'a str>,
    /// Local-space triangles; `None` tests the box alone.
    pub triangles: Option<&'a [[Vec3; 3]]>,
}

impl Occluder<'_> {
    /// Whether `a → b` crosses it: its box, then, for a mesh, a triangle.
    pub fn blocks(&self, a: Vec3, b: Vec3) -> bool {
        let Some(entry) = self.bounds.entry(a, b) else {
            return false;
        };
        let Some(triangles) = self.triangles else {
            return true;
        };
        let (la, lb) = (
            self.bounds.inverse.transform_point3(a),
            self.bounds.inverse.transform_point3(b),
        );
        // From inside a closed mesh its faces point away, so a hit either way counts.
        triangles.iter().any(|t| segment_hits_triangle(la, lb, t))
            || (entry == 0.0 && triangles.iter().any(|t| segment_hits_triangle(lb, la, t)))
    }

    fn ignored(&self, ignore: &HashSet<&str>) -> bool {
        self.id.is_some_and(|id| ignore.contains(id))
    }
}

/// Möller–Trumbore, both faces, between `a` and `b`.
fn segment_hits_triangle(a: Vec3, b: Vec3, [v0, v1, v2]: &[Vec3; 3]) -> bool {
    let d = b - a;
    let (e1, e2) = (*v1 - *v0, *v2 - *v0);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return false;
    }
    let inv = 1.0 / det;
    let s = a - *v0;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    (0.0..=1.0).contains(&(e2.dot(q) * inv))
}

/// The marks for one query: the occluders in the line of sight, and those cut away.
pub fn mark(
    occluders: &[Occluder],
    camera: Vec3,
    input: &OcclusionInput,
) -> (HashSet<Entity>, HashSet<Entity>) {
    let ignore: HashSet<&str> = input.ignore.iter().map(String::as_str).collect();
    let anchor = input.anchor.map(Vec3::from);
    let free = |o: &&Occluder| !o.ignored(&ignore) && !anchor.is_some_and(|a| o.bounds.holds(a));
    let wanted = occluders
        .iter()
        .filter(|o| o.bounds.fadeable)
        .filter(free)
        .filter(|o| input.targets.iter().any(|&t| o.blocks(camera, t.into())))
        .map(|o| o.entity)
        .collect();
    let cut = input.cutaway.map_or_else(HashSet::new, |feet| {
        let feet = Vec3::from(feet);
        let cut_y = occluders
            .iter()
            .filter(free)
            .filter(|o| o.bounds.covers(feet))
            .map(|o| o.bounds.bottom)
            .fold(f32::INFINITY, f32::min);
        if cut_y == f32::INFINITY {
            return HashSet::new();
        }
        occluders
            .iter()
            .filter(free)
            .filter(|o| is_above_cut_plane(o.bounds.bottom, cut_y))
            .filter(|o| {
                let (dx, dz) = (o.bounds.centre.x - feet.x, o.bounds.centre.z - feet.z);
                within_cutaway_radius(dx, dz, o.bounds.radius)
            })
            .map(|o| o.entity)
            .collect()
    });
    (wanted, cut)
}

/// The occupants nothing hides but what they (or the local player) occupy.
pub fn unhidden_occupants(
    occluders: &[Occluder],
    camera: Vec3,
    input: &OcclusionInput,
) -> Vec<String> {
    input
        .occupants
        .iter()
        .filter(|occupant| {
            let ignore: HashSet<&str> = occupant
                .ignore
                .iter()
                .chain(&input.ignore)
                .map(String::as_str)
                .collect();
            let anchor = occupant.anchor.or(input.anchor).map(Vec3::from);
            !occluders.iter().any(|o| {
                o.bounds.fadeable
                    && !o.ignored(&ignore)
                    && !anchor.is_some_and(|a| o.bounds.holds(a))
                    && occupant.targets.iter().any(|&t| o.blocks(camera, t.into()))
            })
        })
        .map(|o| o.id.clone())
        .collect()
}

/// A mesh's local bounds and, when few enough to ray-test, its triangles.
struct MeshShape {
    min: Vec3,
    max: Vec3,
    triangles: Option<Vec<[Vec3; 3]>>,
}

fn mesh_shape(mesh: &Mesh) -> Option<MeshShape> {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let points: Vec<Vec3> = positions.iter().map(|&p| Vec3::from(p)).collect();
    let (min, max) = points
        .iter()
        .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), &p| (lo.min(p), hi.max(p)));
    if points.is_empty() {
        return None;
    }
    let corners = |i: [usize; 3]| Some([*points.get(i[0])?, *points.get(i[1])?, *points.get(i[2])?]);
    let triangles: Vec<[Vec3; 3]> = match mesh.indices() {
        Some(indices) => {
            let indices: Vec<usize> = indices.iter().collect();
            indices
                .chunks_exact(3)
                .filter_map(|c| corners([c[0], c[1], c[2]]))
                .collect()
        }
        None => (0..points.len() / 3)
            .filter_map(|t| corners([t * 3, t * 3 + 1, t * 3 + 2]))
            .collect(),
    };
    Some(MeshShape {
        min,
        max,
        triangles: (triangles.len() <= MAX_RAYCAST_TRIANGLES).then_some(triangles),
    })
}

/// The shapes of the meshes occluders draw, by mesh.
#[derive(Default)]
pub(crate) struct ShapeCache(HashMap<AssetId<Mesh>, Option<MeshShape>>);

/// What a view draws that may occlude.
type Drawn = (
    Entity,
    &'static ChildOf,
    &'static PickTarget,
    &'static Transform,
    &'static Mesh3d,
    &'static MeshMaterial3d<ObjectMaterial>,
    Option<&'static Placed>,
    Option<&'static mut Faded>,
);

pub(crate) fn occlude(
    mut commands: Commands,
    time: Res<Time>,
    mut views: Query<(
        Entity,
        Option<&ViewOcclusion>,
        Option<&ViewCamera>,
        Option<&mut OcclusionMarks>,
        Option<&mut OcclusionStats>,
    )>,
    mut drawn: Query<Drawn>,
    meshes: Res<Assets<Mesh>>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
    mut shapes: Local<ShapeCache>,
) {
    for (view, input, camera, marks, stats) in &mut views {
        let input = input.and_then(|i| i.0.as_ref());
        let (Some(input), Some(camera), Some(mut marks)) = (input, camera, marks) else {
            // Nothing fades without a fading count, so idle views cost nothing.
            if stats.as_ref().is_some_and(|s| s.fading > 0) {
                restore_view(&mut commands, view, &drawn);
            }
            if input.is_some() {
                commands
                    .entity(view)
                    .insert((OcclusionMarks::default(), OcclusionStats::default()));
            } else if stats.is_some_and(|s| *s != OcclusionStats::default()) {
                commands.entity(view).insert(OcclusionStats::default());
            }
            continue;
        };
        let camera_pos = Vec3::from(camera.position);
        let query = marks.frame.is_multiple_of(FRAMES_PER_QUERY);
        marks.frame = marks.frame.wrapping_add(1);
        let mut suppressed = None;
        if query {
            for (_, parent, _, _, mesh, ..) in &drawn {
                if parent.parent() == view && !shapes.0.contains_key(&mesh.id()) {
                    let shape = meshes.get(&mesh.0).and_then(mesh_shape);
                    shapes.0.insert(mesh.id(), shape);
                }
            }
            let occluders: Vec<Occluder> = drawn
                .iter()
                .filter(|(_, parent, ..)| parent.parent() == view)
                .filter_map(|(entity, _, target, transform, mesh, _, placed, _)| {
                    let shape = shapes.0.get(&mesh.id())?.as_ref()?;
                    occluder(entity, target, transform, placed, shape)
                })
                .collect();
            (marks.wanted, marks.cut) = mark(&occluders, camera_pos, input);
            suppressed = Some(unhidden_occupants(&occluders, camera_pos, input));
        }
        let dt = time.delta_secs();
        let (mut fading, mut cut) = (0, 0);
        for (entity, parent, _, _, _, material, _, faded) in &mut drawn {
            if parent.parent() != view {
                continue;
            }
            let wanted = marks.wanted.contains(&entity);
            let is_cut = marks.cut.contains(&entity);
            cut += usize::from(is_cut);
            let current = faded.as_ref().map_or(1.0, |f| f.opacity);
            let next = step_toward(current, fade_target(wanted, is_cut), dt);
            if next >= 1.0 {
                if let Some(faded) = faded {
                    restore(&mut commands, entity, &faded);
                }
                continue;
            }
            fading += 1;
            fade(&mut commands, entity, material, faded, next, &mut materials);
        }
        let mut stats_now = OcclusionStats {
            fading,
            cut,
            silhouette_suppressed: vec![],
        };
        match stats {
            Some(mut stats) => {
                stats_now.silhouette_suppressed =
                    suppressed.unwrap_or_else(|| std::mem::take(&mut stats.silhouette_suppressed));
                stats.set_if_neq(stats_now);
            }
            None => {
                stats_now.silhouette_suppressed = suppressed.unwrap_or_default();
                commands.entity(view).insert(stats_now);
            }
        }
    }
}

/// An object or layout mesh as an occluder; `None` for anything else drawn in the view.
fn occluder<'a>(
    entity: Entity,
    target: &PickTarget,
    transform: &Transform,
    placed: Option<&'a Placed>,
    shape: &'a MeshShape,
) -> Option<Occluder<'a>> {
    // The view's root sits at the origin, so an object's own transform is its world one.
    let world = transform.to_matrix();
    match target {
        PickTarget::Object { .. } => {
            let placed = placed?;
            let named = is_roof_name(&placed.0.kind_id);
            Some(Occluder {
                entity,
                bounds: OccluderBox::new(world, shape.min, shape.max, true, named),
                id: Some(placed.0.id.as_str()),
                triangles: None,
            })
        }
        PickTarget::Layout { .. } => {
            // ponytail: a layout mesh's node name isn't kept, so only an object's kind id names a roof.
            let bounds = OccluderBox::new(world, shape.min, shape.max, shape.triangles.is_some(), false);
            if !bounds.fadeable && bounds.bottom <= MIN_OVERHEAD_BOTTOM {
                return None;
            }
            Some(Occluder {
                entity,
                bounds,
                id: None,
                triangles: shape.triangles.as_deref(),
            })
        }
        _ => None,
    }
}

/// Draws `entity` at `opacity` with a translucent, shadowless copy of its material; hidden at 0.
fn fade(
    commands: &mut Commands,
    entity: Entity,
    material: &MeshMaterial3d<ObjectMaterial>,
    faded: Option<Mut<Faded>>,
    opacity: f32,
    materials: &mut Assets<ObjectMaterial>,
) {
    let fresh = |original: &Handle<ObjectMaterial>, materials: &mut Assets<ObjectMaterial>| {
        let mut ghost = materials.get(original)?.clone();
        ghost.base.alpha_mode = AlphaMode::Blend;
        Some(materials.add(ghost))
    };
    let visibility = if opacity > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    let (ghost, original) = match faded {
        // Something redrew it (a kind reloaded): fade the new material instead.
        Some(mut faded) if material.0 != faded.ghost => {
            let Some(ghost) = fresh(&material.0, materials) else {
                return;
            };
            faded.original = material.0.clone();
            faded.ghost = ghost.clone();
            faded.opacity = opacity;
            (ghost, faded.original.clone())
        }
        Some(mut faded) => {
            faded.opacity = opacity;
            (faded.ghost.clone(), faded.original.clone())
        }
        None => {
            let Some(ghost) = fresh(&material.0, materials) else {
                return;
            };
            let original = material.0.clone();
            commands.entity(entity).insert((
                Faded {
                    original: original.clone(),
                    ghost: ghost.clone(),
                    opacity,
                },
                NotShadowCaster,
            ));
            (ghost, original)
        }
    };
    let alpha = materials.get(&original).map_or(1.0, |m| m.base.base_color.alpha());
    if let Some(mut ghost) = materials.get_mut(&ghost) {
        ghost.base.base_color.set_alpha(alpha * opacity);
    }
    commands
        .entity(entity)
        .insert((MeshMaterial3d(ghost), visibility));
}

fn restore(commands: &mut Commands, entity: Entity, faded: &Faded) {
    commands
        .entity(entity)
        .insert((MeshMaterial3d(faded.original.clone()), Visibility::Inherited))
        .remove::<(Faded, NotShadowCaster)>();
}

fn restore_view(commands: &mut Commands, view: Entity, drawn: &Query<Drawn>) {
    for (entity, parent, .., faded) in drawn.iter() {
        if let Some(faded) = faded
            && parent.parent() == view
        {
            restore(commands, entity, faded);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;

    use crate::tests::{bare_app, camera, scene};

    fn wall(entity: u32, min: [f32; 3], max: [f32; 3], id: &'static str) -> Occluder<'static> {
        Occluder {
            entity: Entity::from_raw_u32(entity).unwrap(),
            bounds: OccluderBox::new(Mat4::IDENTITY, min.into(), max.into(), true, false),
            id: Some(id),
            triangles: None,
        }
    }

    fn input(targets: &[[f32; 3]]) -> OcclusionInput {
        OcclusionInput {
            targets: targets.to_vec(),
            ..default()
        }
    }

    const CAMERA: Vec3 = Vec3::new(0.0, 5.0, 10.0);
    const HEAD: [f32; 3] = [0.0, 1.5, 0.0];

    #[test]
    fn only_what_crosses_the_line_of_sight_fades() {
        let walls = [
            wall(1, [-2.0, 0.0, 4.0], [2.0, 3.0, 4.5], "between"),
            wall(2, [6.0, 0.0, 4.0], [8.0, 3.0, 4.5], "beside"),
            wall(3, [-2.0, 0.0, -4.5], [2.0, 3.0, -4.0], "behind"),
            wall(4, [-2.0, 0.0, 4.0], [2.0, 0.2, 4.5], "rug"),
        ];
        let (wanted, cut) = mark(&walls, CAMERA, &input(&[HEAD]));
        assert_eq!(wanted, HashSet::from([walls[0].entity]));
        assert!(cut.is_empty());
    }

    #[test]
    fn the_seat_ridden_rack_or_pool_the_player_is_in_never_fades() {
        let walls = [wall(1, [-2.0, 0.0, 4.0], [2.0, 3.0, 4.5], "seat")];
        let ignored = OcclusionInput {
            ignore: vec!["seat".into()],
            ..input(&[HEAD])
        };
        assert!(mark(&walls, CAMERA, &ignored).0.is_empty());
        let anchored = OcclusionInput {
            anchor: Some([0.0, 1.0, 4.2]),
            ..input(&[HEAD])
        };
        assert!(mark(&walls, CAMERA, &anchored).0.is_empty());
    }

    #[test]
    fn the_roof_over_the_player_and_everything_above_it_nearby_is_cut() {
        let walls = [
            wall(1, [-10.0, 3.0, -10.0], [10.0, 3.2, 10.0], "roof"),
            wall(2, [-10.0, 3.2, -10.0], [-9.0, 6.0, -9.0], "chimney"),
            wall(3, [-1.0, 0.0, -1.0], [1.0, 2.5, -0.5], "wall"),
            wall(4, [40.0, 3.0, 40.0], [41.0, 4.0, 41.0], "far"),
        ];
        let cutaway = OcclusionInput {
            cutaway: Some([0.0, 0.0, 0.0]),
            ..input(&[])
        };
        let (_, cut) = mark(&walls, CAMERA, &cutaway);
        assert_eq!(cut, HashSet::from([walls[0].entity, walls[1].entity]));
        // Outdoors, nothing is overhead.
        let outdoors = OcclusionInput {
            cutaway: Some([30.0, 0.0, 0.0]),
            ..input(&[])
        };
        assert!(mark(&walls, CAMERA, &outdoors).1.is_empty());
    }

    #[test]
    fn a_mesh_occluder_needs_a_triangle_in_the_way_not_just_its_box() {
        // An L of two triangles filling only the box's left half.
        let triangles = [
            [
                Vec3::new(-2.0, 0.0, 4.2),
                Vec3::new(-0.5, 0.0, 4.2),
                Vec3::new(-2.0, 3.0, 4.2),
            ],
            [
                Vec3::new(-0.5, 0.0, 4.2),
                Vec3::new(-0.5, 3.0, 4.2),
                Vec3::new(-2.0, 3.0, 4.2),
            ],
        ];
        let mut occluder = wall(1, [-2.0, 0.0, 4.0], [2.0, 3.0, 4.5], "mesh");
        occluder.triangles = Some(&triangles);
        assert!(!occluder.blocks(CAMERA, HEAD.into()));
        assert!(occluder.blocks(Vec3::new(-1.5, 2.0, 10.0), Vec3::new(-1.5, 1.0, 0.0)));
    }

    #[test]
    fn an_occupant_hidden_only_by_its_own_seat_shows_no_silhouette() {
        let walls = [
            wall(1, [-2.0, 0.0, 4.0], [2.0, 3.0, 4.5], "sofa"),
            wall(2, [4.0, 0.0, 4.0], [8.0, 3.0, 4.5], "wall"),
        ];
        let occupant = |id: &str, x: f32| Occupant {
            id: id.into(),
            targets: vec![[x, 1.5, 0.0]],
            ignore: vec!["sofa".into()],
            anchor: None,
        };
        let input = OcclusionInput {
            occupants: vec![occupant("on-sofa", 0.0), occupant("behind-wall", 6.0)],
            ..default()
        };
        let camera = Vec3::new(3.0, 5.0, 10.0);
        assert_eq!(unhidden_occupants(&walls, camera, &input), ["on-sofa"]);
    }

    fn view(app: &mut App) -> Entity {
        app.insert_resource(scene(&[("wall", [0.0, 0.0, 2.0]), ("far", [20.0, 0.0, 0.0])]))
            .insert_resource(camera(0.0));
        app.update();
        app.update();
        crate::views::find_view(app.world_mut(), 0).unwrap()
    }

    fn object(app: &mut App, id: &str) -> Entity {
        let mut index = app.world_mut().query::<&crate::ObjectIndex>();
        let index = index.single(app.world()).unwrap();
        index.ids().find(|(i, _)| *i == id).unwrap().1
    }

    #[test]
    fn a_view_fades_the_object_hiding_its_target_and_restores_it_when_cleared() {
        let mut app = bare_app();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(20)));
        let view = view(&mut app);
        let wall = object(&mut app, "wall");
        let centre = app.world().get::<Transform>(wall).unwrap().translation;
        let eye = Vec3::from(camera(0.0).position);
        let behind = centre + (centre - eye).normalize() * 3.0;
        let shared = app
            .world()
            .get::<MeshMaterial3d<ObjectMaterial>>(wall)
            .unwrap()
            .0
            .clone();
        let input = OcclusionInput {
            targets: vec![behind.into()],
            ..default()
        };
        app.world_mut()
            .entity_mut(view)
            .insert((ViewOcclusion(Some(input)), ViewCamera(camera(0.0))));
        for _ in 0..30 {
            app.update();
        }
        let faded = app.world().get::<Faded>(wall).expect("the wall fades");
        assert!((faded.opacity - FADED_OPACITY).abs() < 1e-6);
        let drawn = &app.world().get::<MeshMaterial3d<ObjectMaterial>>(wall).unwrap().0;
        assert_ne!(*drawn, shared);
        let materials = app.world().resource::<Assets<ObjectMaterial>>();
        assert!((materials.get(drawn).unwrap().base.base_color.alpha() - FADED_OPACITY).abs() < 1e-6);
        assert!(app.world().get::<NotShadowCaster>(wall).is_some());
        let far = object(&mut app, "far");
        assert!(app.world().get::<Faded>(far).is_none());
        let stats = app.world().get::<OcclusionStats>(view).unwrap();
        assert_eq!((stats.fading, stats.cut), (1, 0));

        app.world_mut().entity_mut(view).insert(ViewOcclusion(None));
        app.update();
        app.update();
        assert!(app.world().get::<Faded>(wall).is_none());
        assert!(app.world().get::<NotShadowCaster>(wall).is_none());
        assert_eq!(app.world().get::<MeshMaterial3d<ObjectMaterial>>(wall).unwrap().0, shared);
        assert_eq!(app.world().get::<OcclusionStats>(view).unwrap().fading, 0);
    }
}
