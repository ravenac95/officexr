//! Ports of `PokeSpark.tsx`, `SpellBolts.tsx` and `SpellAuras.tsx`: short
//! particle effects, each drawn as a fixed set of unlit spheres placed as a
//! pure function of its age, so the host only says what plays and how old
//! it is, and a frame always draws the same for the same input.
//!
//! The TS versions roll `Math.random()` per frame; here each particle's
//! jitter is a hash of the effect, the particle and its cycle, so the
//! effects shimmer the same way on every replay.

use std::f32::consts::TAU;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use serde::Deserialize;

use crate::avatars::{Avatar, PlayerRender};
use crate::views::{View, ViewPlayers};

/// `PokeSpark.tsx`'s timing.
pub const POKE_SPARK_DELAY_S: f32 = 0.1;
pub const POKE_SPARK_LIFE_S: f32 = 0.55;
/// A bolt's burst outlasts its landing by this long.
pub const IMPACT_S: f32 = 0.9;
const TRAIL: usize = 16;
const EMBERS: usize = 9;
const SMOKE_PUFFS: usize = 6;
const RAYS: usize = 8;
const SPARK_RAYS: usize = 6;
const SPARK_RING: usize = 12;
/// `SpellBolts.tsx` records the head every 22 ms and draws every second record.
const TRAIL_STEP_S: f32 = 0.044;
/// `SpellAuras.tsx`'s head height on an unscaled body.
const HEAD_M: f32 = 1.75;
const SMOKE_PARTICLES: usize = 12;
const SMOKE_LIFE_S: f32 = 1.1;
const SPARKLE_PARTICLES: usize = 18;
const SPARKLE_LIFE_S: f32 = 0.7;

/// A one-off effect, keyed by `id` across frames.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Effect {
    /// A poke's burst at `at`, `age` seconds after the poke.
    PokeSpark { id: u32, at: [f32; 3], age: f32 },
    /// A spell bolt from `origin` along the unit `dir`, landing `distance`
    /// metres on at `speed` m/s, then bursting.
    Bolt {
        id: u32,
        origin: [f32; 3],
        dir: [f32; 3],
        distance: f32,
        speed: f32,
        /// `#rrggbb`.
        color: String,
        /// A fireball's flame, embers and smoke instead of a sparkle's rays.
        fire: bool,
        age: f32,
    },
}

impl Effect {
    fn id(&self) -> u32 {
        match self {
            Effect::PokeSpark { id, .. } | Effect::Bolt { id, .. } => *id,
        }
    }

    fn particle_count(&self) -> usize {
        match self {
            Effect::PokeSpark { .. } => SPARK_RING + SPARK_RAYS,
            Effect::Bolt { .. } => 1 + TRAIL + EMBERS + 1 + SMOKE_PUFFS + RAYS,
        }
    }
}

/// What a spell leaves rising off a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Aura {
    /// A singed head's smoke.
    Smoke,
    /// A colour burst's sparkles.
    Sparkles,
}

/// One sphere of an effect this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub at: Vec3,
    pub radius: f32,
    pub color: Srgba,
    /// Added to what's behind, as a glow, instead of blended over it.
    pub additive: bool,
}


/// A stable pseudo-random number in `[0, 1)` for `(a, b, c)`.
fn hash(a: u32, b: u32, c: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn hex(color: &str) -> Srgba {
    Srgba::hex(color).unwrap_or(Srgba::WHITE)
}

/// Each of the effect's particles at its age; `None` where one doesn't show.
/// The spark's are in its own plane, facing the camera.
pub fn particles(effect: &Effect) -> Vec<Option<Particle>> {
    match effect {
        Effect::PokeSpark { age, .. } => spark(*age),
        Effect::Bolt {
            id,
            origin,
            dir,
            distance,
            speed,
            color,
            fire,
            age,
        } => bolt(
            *id,
            Vec3::from(*origin),
            Vec3::from(*dir),
            *distance,
            *speed,
            hex(color),
            *fire,
            *age,
        ),
    }
}

/// `sparkProgress`: 0 at the end of the delay, 1 at the end of its life.
pub fn spark_progress(age: f32) -> f32 {
    ((age - POKE_SPARK_DELAY_S) / POKE_SPARK_LIFE_S).clamp(0.0, 1.0)
}

fn spark(age: f32) -> Vec<Option<Particle>> {
    let t = spark_progress(age);
    let shown = age >= POKE_SPARK_DELAY_S && t < 1.0;
    let scale = 0.7 + t * 1.1;
    let color = Srgba::hex("fff3a0").unwrap().with_alpha(1.0 - t);
    let ring = (0..SPARK_RING).map(|i| (i as f32 / SPARK_RING as f32, 0.18, 0.02));
    let rays = (0..SPARK_RAYS).map(|i| (i as f32 / SPARK_RAYS as f32, 0.36, 0.035));
    ring.chain(rays)
        .map(|(turn, r, size)| {
            let a = turn * TAU;
            shown.then_some(Particle {
                at: Vec3::new(a.cos() * r, a.sin() * r, 0.0) * scale,
                radius: size * scale,
                color,
                additive: true,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn bolt(
    id: u32,
    origin: Vec3,
    dir: Vec3,
    distance: f32,
    speed: f32,
    color: Srgba,
    fire: bool,
    age: f32,
) -> Vec<Option<Particle>> {
    let travel = if speed > 0.0 { distance / speed } else { 0.0 };
    let flying = age < travel;
    let flown = (age * speed).min(distance);
    let along = |d: f32| origin + dir * d.clamp(0.0, distance);
    let landing = along(distance);
    let impact = age - travel;
    let r = |i: usize, salt: u32| hash(id, i as u32, salt);
    let frame = (age * 60.0) as u32;
    let mut out = Vec::with_capacity(1 + TRAIL + EMBERS + 1 + SMOKE_PUFFS + RAYS);

    let pulse = 1.0 + (age * 50.0).sin() * 0.12 + hash(id, frame, 1) * 0.08;
    out.push(flying.then_some(Particle {
        at: along(flown),
        radius: if fire { 0.2 } else { 0.1 } * pulse,
        color: if fire { hex("#fff2b0") } else { color },
        additive: true,
    }));
    for i in 0..TRAIL {
        let back = i as f32 * TRAIL_STEP_S;
        let shown = flying && i > 0 && age >= back;
        let k = 1.0 - i as f32 / TRAIL as f32;
        let jitter = if fire { 0.05 } else { 0.02 };
        let wobble = Vec3::new(r(i, frame) - 0.5, r(i, frame + 7) - 0.5, r(i, frame + 13) - 0.5);
        let trail_color = match (fire, i) {
            (true, 0..5) => hex("#ffb02e"),
            (true, 5..10) => hex("#ff6a14"),
            (true, _) => hex("#c8300a"),
            (false, _) => color,
        };
        out.push(shown.then_some(Particle {
            at: along((age - back) * speed) + wobble * jitter * 2.0 + Vec3::Y * i as f32 * 0.006,
            radius: if fire { 0.19 } else { 0.06 } * k * (0.75 + r(i, frame + 21) * 0.5),
            color: trail_color,
            additive: true,
        }));
    }
    for i in 0..EMBERS {
        let back = (2 + i * 2) as f32 * TRAIL_STEP_S / 2.0;
        let (sx, sy, sz, ss) = (r(i, 100) - 0.5, r(i, 101), r(i, 102) - 0.5, r(i, 103));
        let shown = flying && fire && age >= back && r(i, frame + 200) > 0.15;
        out.push(shown.then_some(Particle {
            at: along((age - back) * speed)
                + Vec3::new(sx * 0.35, sy * 0.3 + (age * 0.6 + ss) % 0.4, sz * 0.35),
            radius: 0.022 + ss * 0.02,
            color: hex("#ffd45a"),
            additive: true,
        }));
    }
    let k = (impact / 0.42).clamp(0.0, 1.0);
    out.push((!flying && impact < 0.42).then_some(Particle {
        at: landing,
        radius: if fire { 0.35 + k * 1.5 } else { 0.2 + k * 0.7 },
        color: if fire { hex("#fff0b8") } else { color }.with_alpha(1.0 - k),
        additive: true,
    }));
    let k = (impact / IMPACT_S).clamp(0.0, 1.0);
    for i in 0..SMOKE_PUFFS {
        let a = i as f32 / SMOKE_PUFFS as f32 * TAU;
        out.push((fire && !flying && k < 1.0).then_some(Particle {
            at: landing
                + Vec3::new(
                    a.cos() * 0.3 * k,
                    0.1 + k * (0.9 + i as f32 * 0.08),
                    a.sin() * 0.3 * k,
                ),
            radius: 0.18 + k * 0.5,
            color: hex("#3a3532").with_alpha((1.0 - k) * 0.7),
            additive: false,
        }));
    }
    let k = (impact / 0.52).clamp(0.0, 1.0);
    for i in 0..RAYS {
        let a = i as f32 / RAYS as f32 * TAU;
        out.push((!fire && !flying && k < 1.0).then_some(Particle {
            at: landing + Vec3::new(a.cos() * 0.55 * k, a.sin() * 0.55 * k, 0.0),
            radius: 0.06 * (1.0 - k) + 0.01,
            color,
            additive: true,
        }));
    }
    out
}

/// `SpellAuras.tsx`'s particles around a player, relative to its feet, at
/// `time` seconds: a steady stream, each particle on its own cycle.
pub fn aura_particles(aura: Aura, seed: u32, head: f32, scale: f32, lift: f32, time: f32) -> Vec<Option<Particle>> {
    let (count, life) = match aura {
        Aura::Smoke => (SMOKE_PARTICLES, SMOKE_LIFE_S),
        Aura::Sparkles => (SPARKLE_PARTICLES, SPARKLE_LIFE_S),
    };
    (0..count)
        .map(|i| {
            let phase = time / life + i as f32 / count as f32;
            let cycle = phase.floor() as u32;
            let age = phase.fract() * life;
            let k = age / life;
            let r = |salt: u32| hash(seed ^ i as u32, cycle, salt);
            Some(match aura {
                Aura::Smoke => Particle {
                    at: Vec3::new(
                        (r(1) - 0.5) * 0.25 + (r(2) - 0.5) * 0.15 * age,
                        head + (0.55 + r(3) * 0.3) * age,
                        (r(4) - 0.5) * 0.25 + (r(5) - 0.5) * 0.15 * age,
                    ),
                    radius: 0.08 + k * 0.2,
                    color: Srgba::hex("4a4440").unwrap().with_alpha((1.0 - k) * 0.6),
                    additive: false,
                },
                Aura::Sparkles => {
                    let a = r(1) * TAU;
                    Particle {
                        at: Vec3::new(
                            a.cos() * 0.5,
                            (0.2 + r(2) * 1.5) * scale + lift + 0.5 * age,
                            a.sin() * 0.5,
                        ),
                        radius: 0.045 * (1.0 - k) + 0.01,
                        color: Srgba::from(Hsla::new(r(3) * 360.0, 0.95, 0.65, 1.0 - k)),
                        additive: true,
                    }
                }
            })
        })
        .collect()
}

/// An effect's spheres, under its view.
#[derive(Component)]
struct EffectOf(u32);

/// A player's aura spheres, under its avatar.
#[derive(Component)]
struct AuraOf(Aura);

/// One sphere of an effect or aura, in order.
#[derive(Component)]
struct Bead(usize);

#[derive(Resource)]
struct SphereMesh(Handle<Mesh>);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, |mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>| {
        commands.insert_resource(SphereMesh(meshes.add(Sphere::new(1.0).mesh().uv(10, 8))));
    })
    .add_systems(
        Update,
        (sync_effects, sync_auras)
            .in_set(crate::views::SceneSystems)
            .after(crate::views::mirror_resources),
    )
    .add_systems(
        PostUpdate,
        (place_effects, place_auras).before(TransformSystems::Propagate),
    );
}

fn spawn_spheres(
    commands: &mut Commands,
    parent: Entity,
    count: usize,
    mesh: &Handle<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    for i in 0..count {
        commands.spawn((
            Bead(i),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            ChildOf(parent),
        ));
    }
}

/// Spawns each new effect's spheres and despawns finished ones.
fn sync_effects(
    mut commands: Commands,
    views: Query<(Entity, &ViewPlayers), Changed<ViewPlayers>>,
    effects: Query<(Entity, &EffectOf, &ChildOf)>,
    mesh: Res<SphereMesh>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (view, players) in &views {
        for (entity, effect, parent) in &effects {
            if parent.parent() == view && !players.effects.iter().any(|e| e.id() == effect.0) {
                commands.entity(entity).despawn();
            }
        }
        for effect in &players.effects {
            let known = effects
                .iter()
                .any(|(_, e, p)| p.parent() == view && e.0 == effect.id());
            if known {
                continue;
            }
            let entity = commands
                .spawn((
                    EffectOf(effect.id()),
                    Transform::default(),
                    Visibility::default(),
                    ChildOf(view),
                ))
                .id();
            spawn_spheres(&mut commands, entity, effect.particle_count(), &mesh.0, &mut materials);
        }
    }
}

/// Gives each avatar whose player has an aura its spheres, and takes them away after.
fn sync_auras(
    mut commands: Commands,
    views: Query<&ViewPlayers>,
    avatars: Query<(Entity, &Avatar, &ChildOf, Option<&Children>)>,
    auras: Query<&AuraOf>,
    mesh: Res<SphereMesh>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, avatar, view, children) in &avatars {
        let want = views
            .get(view.parent())
            .ok()
            .filter(|p| !p.paused)
            .and_then(|p| p.players.iter().find(|p| p.id == avatar.id()))
            .and_then(|p| p.aura);
        let mut have = None;
        for child in children.into_iter().flatten() {
            if let Ok(aura) = auras.get(*child) {
                if Some(aura.0) == want {
                    have = Some(aura.0);
                } else {
                    commands.entity(*child).despawn();
                }
            }
        }
        if let (Some(aura), None) = (want, have) {
            let count = aura_particles(aura, 0, 0.0, 1.0, 0.0, 0.0).len();
            let parent = commands
                .spawn((AuraOf(aura), Transform::default(), Visibility::default(), ChildOf(entity)))
                .id();
            spawn_spheres(&mut commands, parent, count, &mesh.0, &mut materials);
        }
    }
}

fn show_particles(
    particles: &[Option<Particle>],
    spheres: &Children,
    query: &mut Query<
        (&Bead, &mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>),
        (Without<EffectOf>, Without<AuraOf>),
    >,
    materials: &mut Assets<StandardMaterial>,
) {
    for child in spheres {
        let Ok((sphere, mut transform, mut visibility, material)) = query.get_mut(*child) else {
            continue;
        };
        match particles.get(sphere.0).copied().flatten() {
            Some(p) => {
                *transform = Transform::from_translation(p.at).with_scale(Vec3::splat(p.radius));
                *visibility = Visibility::Inherited;
                if let Some(mut m) = materials.get_mut(&material.0) {
                    let mode = if p.additive {
                        AlphaMode::Add
                    } else {
                        AlphaMode::Blend
                    };
                    if m.base_color != Color::from(p.color) || m.alpha_mode != mode {
                        m.base_color = p.color.into();
                        m.alpha_mode = mode;
                    }
                }
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

#[allow(clippy::type_complexity)]
fn place_effects(
    views: Query<(&View, &ViewPlayers)>,
    cameras: Query<&GlobalTransform>,
    mut effects: Query<(&EffectOf, &ChildOf, &Children, &mut Transform)>,
    mut spheres: Query<
        (&Bead, &mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>),
        (Without<EffectOf>, Without<AuraOf>),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (effect, view, children, mut transform) in &mut effects {
        let Ok((view, players)) = views.get(view.parent()) else {
            continue;
        };
        let Some(e) = players.effects.iter().find(|e| e.id() == effect.0) else {
            continue;
        };
        if let Effect::PokeSpark { at, .. } = e {
            // The spark faces the camera, as `PokeSpark.tsx` copies its quaternion.
            let facing = view
                .camera
                .and_then(|c| cameras.get(c).ok())
                .map_or(Quat::IDENTITY, |c| c.rotation());
            *transform = Transform::from_translation(Vec3::from(*at)).with_rotation(facing);
        }
        show_particles(&particles(e), children, &mut spheres, &mut materials);
    }
}

#[allow(clippy::type_complexity)]
fn place_auras(
    time: Res<Time>,
    views: Query<&ViewPlayers>,
    avatars: Query<(&Avatar, &ChildOf)>,
    auras: Query<(&AuraOf, &ChildOf, &Children)>,
    mut spheres: Query<
        (&Bead, &mut Transform, &mut Visibility, &MeshMaterial3d<StandardMaterial>),
        (Without<EffectOf>, Without<AuraOf>),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (aura, avatar, children) in &auras {
        let Ok((avatar, view)) = avatars.get(avatar.parent()) else {
            continue;
        };
        let Some(p) = views
            .get(view.parent())
            .ok()
            .and_then(|v| v.players.iter().find(|p| p.id == avatar.id()))
        else {
            continue;
        };
        let head = HEAD_M * p.body.scale + p.body.lift;
        let particles = aura_particles(aura.0, seed_of(p), head, p.body.scale, p.body.lift, time.elapsed_secs());
        show_particles(&particles, children, &mut spheres, &mut materials);
    }
}

fn seed_of(p: &PlayerRender) -> u32 {
    p.id.bytes().fold(2_166_136_261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16_777_619))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bolt_at(age: f32, fire: bool) -> Vec<Option<Particle>> {
        particles(&Effect::Bolt {
            id: 1,
            origin: [0.0; 3],
            dir: [1.0, 0.0, 0.0],
            distance: 10.0,
            speed: 20.0,
            color: "#66ccff".into(),
            fire,
            age,
        })
    }

    #[test]
    fn a_bolts_head_flies_to_its_landing_then_bursts() {
        let head = |age| bolt_at(age, false)[0];
        assert!(head(0.25).unwrap().at.distance(Vec3::X * 5.0) < 1e-4);
        assert!(head(0.6).is_none(), "landed");
        let flash = |age| bolt_at(age, false)[1 + TRAIL + EMBERS];
        assert!(flash(0.4).is_none(), "still flying");
        let burst = flash(0.6).unwrap();
        assert_eq!(burst.at, Vec3::X * 10.0);
        assert!(burst.color.alpha < 1.0);
        assert!(bolt_at(0.5 + IMPACT_S, false).iter().all(Option::is_none), "done");
    }

    #[test]
    fn a_fireball_trails_embers_and_leaves_smoke_where_a_sparkle_leaves_rays() {
        let shown = |ps: &[Option<Particle>], range: std::ops::Range<usize>| {
            ps[range].iter().filter(|p| p.is_some()).count()
        };
        let smoke = 1 + TRAIL + EMBERS + 1;
        let rays = smoke + SMOKE_PUFFS;
        let fire = bolt_at(0.7, true);
        assert_eq!(shown(&fire, smoke..rays), SMOKE_PUFFS);
        assert_eq!(shown(&fire, rays..rays + RAYS), 0);
        let sparkle = bolt_at(0.7, false);
        assert_eq!(shown(&sparkle, smoke..rays), 0);
        assert_eq!(shown(&sparkle, rays..rays + RAYS), RAYS);
        assert!(shown(&bolt_at(0.3, true), 1 + TRAIL..1 + TRAIL + EMBERS) > 0);
        assert_eq!(fire, bolt_at(0.7, true), "the same age draws the same");
    }

    #[test]
    fn a_spark_waits_grows_and_fades() {
        let spark = |age| particles(&Effect::PokeSpark { id: 3, at: [0.0; 3], age });
        assert!(spark(0.05).iter().all(Option::is_none));
        let early = spark(0.15)[0].unwrap();
        let late = spark(0.5)[0].unwrap();
        assert!(late.radius > early.radius && late.color.alpha < early.color.alpha);
        assert!(spark(0.7).iter().all(Option::is_none));
        assert_eq!(spark_progress(0.1), 0.0);
        assert_eq!(spark_progress(0.7), 1.0);
    }

    #[test]
    fn smoke_rises_from_the_head_and_sparkles_ring_the_body() {
        let smoke = aura_particles(Aura::Smoke, 7, 1.75, 1.0, 0.0, 3.3);
        assert!(smoke.iter().flatten().all(|p| p.at.y >= 1.75 && p.at.y < 1.75 + 1.0));
        let sparkles = aura_particles(Aura::Sparkles, 7, 1.75, 1.0, 0.0, 3.3);
        assert!(sparkles.iter().flatten().all(|p| (p.at.xz().length() - 0.5).abs() < 1e-4));
        assert_ne!(smoke, aura_particles(Aura::Smoke, 7, 1.75, 1.0, 0.0, 3.4));
    }

    #[test]
    fn effects_deserialise_as_the_host_sends_them() {
        let e: Effect = serde_json::from_value(serde_json::json!({
            "kind": "pokeSpark", "id": 4, "at": [1, 2, 3], "age": 0.2
        }))
        .unwrap();
        assert_eq!(e, Effect::PokeSpark { id: 4, at: [1.0, 2.0, 3.0], age: 0.2 });
    }
}
