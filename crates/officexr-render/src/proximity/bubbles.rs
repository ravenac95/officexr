//! `BubbleParticles.tsx`: soft bubbles rising in a ring around each
//! approaching player's disc and each meeting area's rim, wrapping back to
//! the floor at the float height.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::{GlowMaterial, Glows, ViewProximity};
use crate::views::View;

/// Bubbles per player or area.
const COUNT: usize = 120;
/// `PointsMaterial.size`, before the style's multiplier: with size
/// attenuation a point that size spans `size · tan(fov / 2)` metres.
const POINT_SIZE: f32 = 0.45;
const OPACITY: f32 = 0.95;
/// Rise phase per second, per unit of a bubble's speed.
const RISE_RATE: f64 = 0.18;
/// Each bubble's angle sways this many radians, at this many per second.
const WOBBLE: f32 = 0.04;
const WOBBLE_RATE: f64 = 0.2;

/// One bubble's constants, as `BubbleParticles` seeds them on mount.
struct Seed {
    angle: f32,
    /// 0 at the ring's inner edge, 1 at its outer.
    radial: f32,
    /// 0.4 to 1.2.
    speed: f32,
    /// The rise phase at time 0.
    y: f32,
    wobble: f32,
}

pub(crate) struct Emitter {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<GlowMaterial>,
    seeds: Vec<Seed>,
}

/// The view's bubble rings, by `player:<id>` or `area:<id>`.
#[derive(Component, Default)]
pub(crate) struct Bubbles(HashMap<String, Emitter>);

/// splitmix64, seeded by the emitter's key, so a ring's bubbles are the
/// same from run to run.
struct Rng(u64);

impl Rng {
    fn new(key: &str) -> Self {
        // FNV-1a.
        Self(key.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        }))
    }

    /// In [0, 1).
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// A disc's bubbles spread evenly over the ring's area; an area's evenly
/// across its thin band.
fn seeds(key: &str, disc: bool) -> Vec<Seed> {
    let mut rng = Rng::new(key);
    (0..COUNT)
        .map(|_| {
            let angle = rng.next() * TAU;
            let radial = rng.next();
            Seed {
                angle,
                radial: if disc { radial.sqrt() } else { radial },
                speed: 0.4 + rng.next() * 0.8,
                y: rng.next(),
                wobble: rng.next() * TAU,
            }
        })
        .collect()
}

/// `COUNT` quads, placed every frame.
fn pool_mesh() -> Mesh {
    let corners = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let uvs: Vec<[f32; 2]> = (0..COUNT).flat_map(|_| corners).collect();
    let indices = (0..COUNT as u32)
        .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i))
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; COUNT * 4])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; COUNT * 4])
    .with_inserted_indices(Indices::U32(indices))
}

/// A ring's place this frame.
struct Ring {
    center: Vec3,
    inner: f32,
    outer: f32,
    color: Color,
    disc: bool,
}

pub(crate) fn sync_bubbles(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GlowMaterial>>,
    mut views: Query<(Entity, &View, &ViewProximity, &Glows, Option<&mut Bubbles>)>,
    cameras: Query<(&GlobalTransform, &Projection)>,
) {
    let t = time.elapsed_secs_f64();
    for (view, v, proximity, glows, bubbles) in &mut views {
        let mut fresh = Bubbles::default();
        let bubbles = match bubbles {
            Some(b) => b.into_inner(),
            None => &mut fresh,
        };
        let style = &proximity.style;
        let inset = style.meeting_border_inset;
        let outset = style.meeting_border_outset;
        let mut rings: HashMap<String, Ring> = HashMap::new();
        for id in &proximity.bubbles {
            if let Some(center) = glows.discs.get(id).and_then(|g| g.at) {
                let r = style.disc_radius;
                rings.insert(
                    format!("player:{id}"),
                    Ring {
                        center,
                        inner: (r - inset).max(0.01),
                        outer: r + outset.max(0.0),
                        color: style.entering_color,
                        disc: true,
                    },
                );
            }
        }
        for (id, glow) in &glows.areas {
            if let Some(center) = glow.at {
                rings.insert(
                    format!("area:{id}"),
                    Ring {
                        center,
                        inner: (glow.radius - inset).max(0.01),
                        outer: glow.radius + outset,
                        color: style.entered_color,
                        disc: false,
                    },
                );
            }
        }

        bubbles.0.retain(|key, emitter| {
            let kept = rings.contains_key(key);
            if !kept {
                commands.entity(emitter.entity).despawn();
            }
            kept
        });
        let Some((camera, projection)) = v.camera.and_then(|c| cameras.get(c).ok()) else {
            continue;
        };
        let tan_half_fov = match projection {
            Projection::Perspective(p) => (p.fov / 2.0).tan(),
            _ => 1.0,
        };
        let half = POINT_SIZE * style.sparkle_size * tan_half_fov / 2.0;
        let (right, up) = (camera.right() * half, camera.up() * half);
        let corners = [-right + up, right + up, right - up, -right - up];

        for (key, ring) in &rings {
            let emitter = bubbles.0.entry(key.clone()).or_insert_with(|| {
                let mesh = meshes.add(pool_mesh());
                let material = materials.add(GlowMaterial::default());
                Emitter {
                    entity: commands
                        .spawn((
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::default(),
                            Visibility::default(),
                            NotShadowCaster,
                            // The quads move every frame; the spawn-time bounds don't hold.
                            NoFrustumCulling,
                            ChildOf(view),
                        ))
                        .id(),
                    mesh,
                    material,
                    seeds: seeds(key, ring.disc),
                }
            });
            let color = (ring.color.to_linear() * OPACITY).with_alpha(OPACITY);
            if materials
                .get(&emitter.material)
                .is_some_and(|m| m.color != color)
            {
                materials.get_mut(&emitter.material).unwrap().color = color;
            }
            let Some(mut mesh) = meshes.get_mut(&emitter.mesh) else {
                continue;
            };
            let band = ring.outer - ring.inner;
            let positions: Vec<[f32; 3]> = emitter
                .seeds
                .iter()
                .flat_map(|s| {
                    let sway = (t * WOBBLE_RATE + f64::from(s.wobble)).sin() as f32 * WOBBLE;
                    let angle = s.angle + sway;
                    let r = ring.inner + s.radial * band;
                    let rise =
                        f64::from(s.y) + t * f64::from(s.speed * style.sparkle_speed) * RISE_RATE;
                    let y = rise.fract() as f32 * style.sparkle_float_height;
                    let at = ring.center + Vec3::new(angle.cos() * r, y, angle.sin() * r);
                    corners.map(|c| (at + c).to_array())
                })
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        }

        if !fresh.0.is_empty() {
            commands.entity(view).insert(fresh);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::time::TimeUpdateStrategy;
    use serde_json::json;

    use super::*;
    use crate::proximity::GlowStyle;
    use crate::tests::{bare_app, camera, scene};
    use crate::views::{ViewHub, ViewId, ViewKind, ViewMsg, ViewTarget, find_view};
    use crate::{PlayerRender, Players, WorldDiff};

    fn app() -> (App, ViewId) {
        let mut app = bare_app();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            1.0 / 60.0,
        )));
        let hub = app.world().resource::<ViewHub>().clone();
        let id = hub.create(ViewKind::World, |_| ViewTarget::Image(UVec2::new(8, 8)));
        let mut diff: WorldDiff =
            serde_json::from_str(include_str!("../../examples/booper.scene.json")).unwrap();
        diff.kinds = Some(vec![]);
        diff.objects = scene(&[]).objects.into();
        hub.send(id, ViewMsg::World(Box::new(diff)));
        hub.send(id, ViewMsg::Camera(camera(0.0)));
        let players = ["a", "b"]
            .iter()
            .enumerate()
            .map(|(i, id)| PlayerRender {
                id: id.to_string(),
                character: "Barbarian".into(),
                pos: Vec3::new(i as f32 * 4.0, 0.0, 0.0),
                yaw: 0.0,
                vel: Vec3::ZERO,
                name: String::new(),
                airborne: false,
            })
            .collect();
        hub.send(
            id,
            ViewMsg::Players(Players {
                players,
                ..default()
            }),
        );
        (app, id)
    }

    fn set(app: &mut App, id: ViewId, proximity: serde_json::Value) {
        let hub = app.world().resource::<ViewHub>().clone();
        assert!(hub.send(
            id,
            ViewMsg::Proximity(serde_json::from_value(proximity).unwrap())
        ));
        for _ in 0..10 {
            app.update();
        }
    }

    /// Each ring's bubble centres, by key.
    fn rings(app: &mut App, id: ViewId) -> HashMap<String, Vec<Vec3>> {
        let view = find_view(app.world_mut(), id).unwrap();
        let Some(bubbles) = app.world().get::<Bubbles>(view) else {
            return HashMap::new();
        };
        let meshes = app.world().resource::<Assets<Mesh>>();
        bubbles
            .0
            .iter()
            .map(|(key, emitter)| {
                let mesh = meshes.get(&emitter.mesh).unwrap();
                let quads = mesh
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .unwrap()
                    .as_float3()
                    .unwrap()
                    .chunks(4)
                    .map(|q| q.iter().map(|&c| Vec3::from(c)).sum::<Vec3>() / 4.0)
                    .collect();
                (key.clone(), quads)
            })
            .collect()
    }

    #[test]
    fn bubbles_rise_in_a_ring_round_each_approacher_and_area() {
        let (mut app, id) = app();
        set(
            &mut app,
            id,
            json!({
                "players": {"a": "entering"},
                "bubbles": ["a"],
                "areas": [{"id": "m", "center": [4, 0], "radius": 2.5, "members": ["b"]}],
            }),
        );
        let rings = rings(&mut app, id);
        let mut keys: Vec<_> = rings.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["area:m", "player:a"]);
        let style = GlowStyle::default();
        let feet = Players::default().feet(&PlayerRender {
            id: String::new(),
            character: String::new(),
            pos: Vec3::ZERO,
            yaw: 0.0,
            vel: Vec3::ZERO,
            name: String::new(),
            airborne: false,
        });
        for (key, center, r) in [("player:a", 0.0, style.disc_radius), ("area:m", 4.0, 2.5)] {
            let bubbles = &rings[key];
            assert_eq!(bubbles.len(), COUNT);
            for b in bubbles {
                let d = Vec2::new(b.x - center, b.z).length();
                assert!(
                    (r - 0.06..=r + 0.06).contains(&d),
                    "{key}: {d} from the centre"
                );
                let height = b.y - feet.y;
                assert!((0.0..=style.sparkle_float_height + 0.05).contains(&height));
            }
        }
    }

    #[test]
    fn a_ring_goes_when_its_approach_or_area_does() {
        let (mut app, id) = app();
        set(
            &mut app,
            id,
            json!({
                "bubbles": ["a"],
                "areas": [{"id": "m", "center": [4, 0], "radius": 2.5, "members": ["b"]}],
            }),
        );
        assert_eq!(rings(&mut app, id).len(), 2);
        set(&mut app, id, json!({}));
        assert!(rings(&mut app, id).is_empty());
        let mut emitters = app
            .world_mut()
            .query_filtered::<(), (With<NoFrustumCulling>, With<MeshMaterial3d<GlowMaterial>>)>();
        assert_eq!(emitters.iter(app.world()).count(), 0);
    }
}
