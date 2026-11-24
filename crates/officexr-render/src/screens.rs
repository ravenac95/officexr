//! Port of `screenFace.ts` and `useDeskScreenGlow.ts`: a desk's monitor
//! lights while someone sits at it. The host decides which objects are
//! screens, which way their chair is and how bright each glows
//! (`setScreens`); the view finds the model's broad near-vertical faces and
//! lights the one facing the chair with an additive glow laid over it.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use serde::Deserialize;

use crate::ObjectIndex;

/// At most this many candidate faces per model.
pub const MAX_SCREEN_FACES: usize = 6;
/// A face whose normal's |y| is above this isn't near-vertical: not a screen.
pub const NEAR_VERTICAL_MAX_NY: f32 = 0.26;
/// The cosine a face's normal must reach toward the chair.
pub const FACING_MIN_DOT: f32 = 0.5;
/// Candidates are at least this share of the biggest face, which leaves the sides out.
pub const MIN_FACE_AREA_RATIO: f32 = 0.25;
/// `screenGlowShader.ts`'s glow colour.
const GLOW: Srgba = Srgba::rgb(0x8f as f32 / 255.0, 0xb8 as f32 / 255.0, 1.0);
/// Lifts the glow off the face it covers, in the model's units.
const LIFT: f32 = 0.002;

/// One screen as `setScreens` takes it.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScreenInput {
    /// The object instance the screen is.
    pub id: String,
    /// 0 off to 1 fully on; the host steps the power-up flicker.
    pub glow: f32,
    /// World XZ from the screen toward its chair: the face that points this way lights.
    pub toward: [f32; 2],
}

/// The screens a view lights.
#[derive(Component, Default, Deref)]
pub struct ViewScreens(pub Vec<ScreenInput>);

/// A broad, flat, near-vertical face of a model.
#[derive(Clone, Debug)]
pub struct ScreenFace {
    /// Local XZ normal, unit.
    pub normal: Vec2,
    pub area: f32,
    pub triangles: Vec<[Vec3; 3]>,
}

/// A plane's summed area, area-weighted XZ normal and triangles.
type Cluster = (f32, Vec2, Vec<[Vec3; 3]>);

/// `findScreenFaces`: the model's largest coplanar near-vertical faces, biggest first.
pub fn find_screen_faces(triangles: &[[Vec3; 3]]) -> Vec<ScreenFace> {
    let (lo, hi) = triangles
        .iter()
        .flatten()
        .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), &p| {
            (lo.min(p), hi.max(p))
        });
    let plane_step = (hi - lo).max_element() * 0.01;
    let mut clusters: HashMap<(i32, i32, i32), Cluster> = HashMap::new();
    for t @ [a, b, c] in triangles {
        let n = (*b - *a).cross(*c - *a);
        let area = n.length() / 2.0;
        if area < 1e-12 {
            continue;
        }
        let n = n.normalize();
        if n.y.abs() > NEAR_VERTICAL_MAX_NY {
            continue;
        }
        let key = (
            (n.x * 20.0).round() as i32,
            (n.z * 20.0).round() as i32,
            (n.dot(*a) / plane_step).round() as i32,
        );
        let cluster = clusters.entry(key).or_default();
        cluster.0 += area;
        cluster.1 += Vec2::new(n.x, n.z) * area;
        cluster.2.push(*t);
    }
    let mut ranked: Vec<_> = clusters.into_values().collect();
    ranked.sort_by(|p, q| q.0.total_cmp(&p.0));
    let Some(biggest) = ranked.first().map(|c| c.0) else {
        return vec![];
    };
    ranked
        .into_iter()
        .take(MAX_SCREEN_FACES)
        .filter(|c| c.0 >= biggest * MIN_FACE_AREA_RATIO)
        .map(|(area, normal, triangles)| ScreenFace {
            normal: normal.try_normalize().unwrap_or(Vec2::Y),
            area,
            triangles,
        })
        .collect()
}

/// `chooseScreenFace`: the biggest face whose normal, turned by `rotation`,
/// points within 60° of `toward` (world XZ); `None` when none does.
pub fn choose_screen_face(faces: &[ScreenFace], rotation: Quat, toward: Vec2) -> Option<usize> {
    let toward = toward.try_normalize()?;
    faces
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            let world = rotation * Vec3::new(f.normal.x, 0.0, f.normal.y);
            Vec2::new(world.x, world.z).dot(toward) >= FACING_MIN_DOT
        })
        .max_by(|(_, p), (_, q)| p.area.total_cmp(&q.area))
        .map(|(i, _)| i)
}

/// A mesh's triangles in its own space; empty when it has no float positions.
pub(crate) fn mesh_triangles(mesh: &Mesh) -> Vec<[Vec3; 3]> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return vec![];
    };
    let at = |i: usize| positions.get(i).copied().map(Vec3::from);
    let corners = |i: [usize; 3]| Some([at(i[0])?, at(i[1])?, at(i[2])?]);
    match mesh.indices() {
        Some(indices) => {
            let indices: Vec<usize> = indices.iter().collect();
            indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|&c| corners(c))
                .collect()
        }
        None => (0..positions.len() / 3)
            .filter_map(|t| corners([t * 3, t * 3 + 1, t * 3 + 2]))
            .collect(),
    }
}

/// The face's triangles lifted off it along its normal, both sides wound.
fn glow_mesh(face: &ScreenFace) -> Mesh {
    let lift = Vec3::new(face.normal.x, 0.0, face.normal.y) * LIFT;
    let positions: Vec<Vec3> = face
        .triangles
        .iter()
        .flat_map(|t| t.map(|p| p + lift))
        .collect();
    let normals = vec![Vec3::new(face.normal.x, 0.0, face.normal.y); positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
}

fn glow_colour(glow: f32) -> Color {
    let linear = LinearRgba::from(GLOW) * glow.clamp(0.0, 1.0);
    linear.with_alpha(1.0).into()
}

/// One lit screen: its glow entity, and what it was built from.
struct Lit {
    object: Entity,
    glow_entity: Entity,
    mesh: AssetId<Mesh>,
    face: usize,
    material: Handle<StandardMaterial>,
    glow: f32,
}

/// A view's lit screens, by object id.
#[derive(Component, Default)]
pub struct ScreenIndex(HashMap<String, Lit>);

impl ScreenIndex {
    /// The ids of the screens drawn lit.
    pub fn lit(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }
}

/// The candidate faces of each screen model, by mesh.
#[derive(Default)]
pub(crate) struct FaceCache(HashMap<AssetId<Mesh>, Vec<ScreenFace>>);

/// Lights, dims and puts out each view's screens. Runs every frame while a
/// view has screens: a glow ramps over several.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_screens(
    mut commands: Commands,
    mut views: Query<(Entity, &ViewScreens, &ObjectIndex, Option<&mut ScreenIndex>)>,
    objects: Query<(&Mesh3d, &Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut faces: Local<FaceCache>,
) {
    for (view, screens, objects_index, index) in &mut views {
        let Some(mut index) = index else {
            commands.entity(view).insert(ScreenIndex::default());
            continue;
        };
        let ids: HashMap<&str, Entity> = objects_index.ids().collect();
        let mut keep: Vec<String> = vec![];
        for screen in screens.iter().filter(|s| s.glow > 0.0) {
            let Some(&object) = ids.get(screen.id.as_str()) else {
                continue;
            };
            let Ok((mesh, transform)) = objects.get(object) else {
                continue;
            };
            let candidates = faces.0.entry(mesh.id()).or_insert_with(|| {
                meshes
                    .get(&mesh.0)
                    .map(|m| find_screen_faces(&mesh_triangles(m)))
                    .unwrap_or_default()
            });
            let Some(face) =
                choose_screen_face(candidates, transform.rotation, Vec2::from(screen.toward))
            else {
                continue;
            };
            keep.push(screen.id.clone());
            if let Some(lit) = index.0.get_mut(&screen.id)
                && lit.object == object
                && lit.mesh == mesh.id()
                && lit.face == face
            {
                if lit.glow != screen.glow {
                    lit.glow = screen.glow;
                    if let Some(mut m) = materials.get_mut(&lit.material) {
                        m.base_color = glow_colour(screen.glow);
                    }
                }
                continue;
            }
            if let Some(old) = index.0.remove(&screen.id) {
                commands.entity(old.glow_entity).try_despawn();
            }
            let material = materials.add(StandardMaterial {
                base_color: glow_colour(screen.glow),
                unlit: true,
                alpha_mode: AlphaMode::Add,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            let glow_entity = commands
                .spawn((
                    Mesh3d(meshes.add(glow_mesh(&candidates[face]))),
                    MeshMaterial3d(material.clone()),
                    Transform::IDENTITY,
                    NotShadowCaster,
                    ChildOf(object),
                ))
                .id();
            index.0.insert(
                screen.id.clone(),
                Lit {
                    object,
                    glow_entity,
                    mesh: mesh.id(),
                    face,
                    material,
                    glow: screen.glow,
                },
            );
        }
        index.0.retain(|id, lit| {
            let lit_still = keep.contains(id);
            if !lit_still {
                commands.entity(lit.glow_entity).try_despawn();
            }
            lit_still
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cuboid(size: Vec3, at: Vec3) -> Vec<[Vec3; 3]> {
        mesh_triangles(&Cuboid::from_size(size).mesh().build().translated_by(at))
    }

    /// `screenFace.test.ts`'s monitor: a base, a neck and a 0.6 × 0.35 m body.
    /// The neck is thinner than the body here: a face within a plane step
    /// (1% of the model) of the body's front clusters with it, and 4 cm
    /// lands on that rounding edge.
    fn monitor() -> Vec<[Vec3; 3]> {
        [
            cuboid(Vec3::new(0.2, 0.02, 0.15), Vec3::new(0.0, 0.01, 0.0)),
            cuboid(Vec3::new(0.04, 0.2, 0.01), Vec3::new(0.0, 0.12, 0.0)),
            cuboid(Vec3::new(0.6, 0.35, 0.03), Vec3::new(0.0, 0.32, 0.0)),
        ]
        .concat()
    }

    #[test]
    fn the_broad_faces_of_the_body_rank_first() {
        let faces = find_screen_faces(&monitor());
        let [front, back] = [&faces[0], &faces[1]];
        assert!((front.normal.y.abs() - 1.0).abs() < 1e-5);
        assert_eq!(front.normal.y, -back.normal.y);
        assert!((front.area - 0.6 * 0.35).abs() < 1e-3);
        // Every triangle of the front lies in the body's front plane.
        for p in front.triangles.iter().flatten() {
            assert!((p.z - front.normal.y * 0.015).abs() < 1e-5);
        }
    }

    #[test]
    fn a_flat_horizontal_model_has_no_face() {
        let floor = mesh_triangles(&Plane3d::default().mesh().size(1.0, 1.0).build());
        assert!(find_screen_faces(&floor).is_empty());
    }

    #[test]
    fn the_face_pointing_at_the_chair_lights_and_follows_the_yaw() {
        let faces = find_screen_faces(&monitor());
        let front = faces.iter().position(|f| f.normal.y > 0.0);
        let back = faces.iter().position(|f| f.normal.y < 0.0);
        let at = |yaw: f32, x: f32, z: f32| {
            choose_screen_face(&faces, Quat::from_rotation_y(yaw), Vec2::new(x, z))
        };
        assert_eq!(at(0.0, 0.0, 2.0), front);
        assert_eq!(at(0.0, 0.0, -2.0), back);
        assert_eq!(at(std::f32::consts::PI, 0.0, 2.0), back);
        assert_eq!(at(std::f32::consts::FRAC_PI_2, 2.0, 0.0), front);
        assert_eq!(at(0.0, 2.0, 0.0), None);
        assert_eq!(at(0.0, 0.0, 0.0), None);
    }

    #[test]
    fn a_view_lights_a_screen_dims_it_and_puts_it_out() {
        use crate::tests::{bare_app, camera, scene};
        let mut app = bare_app();
        app.insert_resource(scene(&[("monitor", [0.0; 3])]))
            .insert_resource(camera(0.0));
        app.update();
        app.update();
        let view = crate::views::find_view(app.world_mut(), 0).unwrap();
        let screen = |glow: f32| ScreenInput {
            id: "monitor".into(),
            glow,
            toward: [0.0, 1.0],
        };
        app.world_mut()
            .entity_mut(view)
            .insert(ViewScreens(vec![screen(0.5)]));
        app.update();
        app.update();
        let lit = |app: &mut App| -> Vec<String> {
            let mut q = app.world_mut().query::<&ScreenIndex>();
            q.single(app.world())
                .unwrap()
                .lit()
                .map(String::from)
                .collect()
        };
        assert_eq!(lit(&mut app), ["monitor"]);
        let mut glows = app
            .world_mut()
            .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<NotShadowCaster>>();
        let handle = glows.single(app.world()).unwrap().0.clone();
        let colour = |app: &App| {
            let m = app.world().resource::<Assets<StandardMaterial>>();
            m.get(&handle).unwrap().base_color.to_linear().blue
        };
        let half = colour(&app);

        app.world_mut()
            .entity_mut(view)
            .insert(ViewScreens(vec![screen(1.0)]));
        app.update();
        assert!((colour(&app) - 2.0 * half).abs() < 1e-5);

        app.world_mut()
            .entity_mut(view)
            .insert(ViewScreens(vec![screen(0.0)]));
        app.update();
        assert!(lit(&mut app).is_empty());
        assert_eq!(glows.iter(app.world()).count(), 0);
    }
}
