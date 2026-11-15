//! `ProximityGlow.tsx`'s conversation glow: a pulsing ground disc under
//! each player entering or leaving a conversation, and a steady one over
//! each meeting area, with [`bubbles`] rising around both.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use serde::Deserialize;

use crate::scene::hex;
use crate::views::ViewPlayers;

mod bubbles;
pub(crate) use bubbles::sync_bubbles;

/// How fast opacities and area radii ease toward their targets, per second.
const EASE_RATE: f32 = 6.0;
/// A meeting area glows a touch dimmer than a player's disc at its peak.
const AREA_INTENSITY: f32 = 0.85;
/// Above the feet, so the disc clears the floor it lies on. three.js lifts
/// it 0.15 m off the body root, which now sits below the feet.
const LIFT: f32 = 0.02;
const DISC_SEGMENTS: u32 = 64;
const AREA_SEGMENTS: u32 = 96;
/// Below this a disc draws nothing worth a draw call.
const MIN_OPACITY: f32 = 1e-3;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiscState {
    Entering,
    Entered,
    Exiting,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingArea {
    pub id: String,
    /// XZ.
    pub center: [f32; 2],
    pub radius: f32,
    pub members: Vec<String>,
}

/// The world constants' `proximity` tunables the glow reads.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct GlowStyle {
    pub disc_radius: f32,
    /// Pulses per second.
    pub pulse_speed: f32,
    /// Peak opacity.
    pub intensity: f32,
    #[serde(deserialize_with = "hex")]
    pub entering_color: Color,
    #[serde(deserialize_with = "hex")]
    pub entered_color: Color,
    #[serde(deserialize_with = "hex")]
    pub exiting_color: Color,
    /// How far inside a disc's or area's rim its bubble ring starts, in metres.
    pub meeting_border_inset: f32,
    /// How far outside the rim the ring ends.
    pub meeting_border_outset: f32,
    /// The bubbles' rise speed multiplier.
    pub sparkle_speed: f32,
    /// How high bubbles rise, in metres, before starting again from the floor.
    pub sparkle_float_height: f32,
    /// The bubbles' size multiplier, with the camera mode's already applied.
    pub sparkle_size: f32,
}

impl Default for GlowStyle {
    /// `DEFAULT_WORLD_CONSTANTS.proximity`.
    fn default() -> Self {
        Self {
            disc_radius: 1.6,
            pulse_speed: 0.8,
            intensity: 1.0,
            entering_color: Srgba::hex("#ffd24a").unwrap().into(),
            entered_color: Srgba::hex("#7be67b").unwrap().into(),
            exiting_color: Srgba::hex("#ff8c42").unwrap().into(),
            meeting_border_inset: 0.05,
            meeting_border_outset: 0.05,
            sparkle_speed: 1.5,
            sparkle_float_height: 1.5,
            sparkle_size: 1.0,
        }
    }
}

/// One frame's proximity output, as `TrackerOutput` holds it.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Proximity {
    /// Each player's disc state; a player left out shows no disc.
    pub players: HashMap<String, DiscState>,
    pub areas: Vec<MeetingArea>,
    /// The feet of players the view doesn't draw, as the first-person local
    /// player; the drawn ones stand where `setPlayers` put them.
    pub undrawn: HashMap<String, [f32; 3]>,
    /// The players bubbles rise around: both ends of each of the local
    /// player's `entering` pairs.
    pub bubbles: Vec<String>,
    pub style: GlowStyle,
}

/// What a view's glow should show.
#[derive(Component, Deref, Default)]
pub struct ViewProximity(pub Proximity);

/// `radialTexture()` on a circle, blended additively and not tone mapped,
/// as `ProximityGlow`'s `meshBasicMaterial`.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub struct GlowMaterial {
    /// Premultiplied by the opacity.
    #[uniform(0)]
    color: LinearRgba,
}

impl Material for GlowMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://officexr_render/proximity.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
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

struct Glow {
    entity: Entity,
    opacity: f32,
    /// Eased; areas only.
    radius: f32,
    /// The centre of its disc this frame; `None` for an area with no member to stand on.
    at: Option<Vec3>,
}

/// The view's glow entities, by player and area id.
#[derive(Component, Default)]
pub(crate) struct Glows {
    discs: HashMap<String, Glow>,
    areas: HashMap<String, Glow>,
}

#[derive(Resource)]
pub(crate) struct GlowMeshes {
    disc: Handle<Mesh>,
    area: Handle<Mesh>,
}

impl FromWorld for GlowMeshes {
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let mut unit = |segments| meshes.add(Circle::new(1.0).mesh().resolution(segments).build());
        Self {
            disc: unit(DISC_SEGMENTS),
            area: unit(AREA_SEGMENTS),
        }
    }
}

fn ease(current: f32, target: f32, dt: f32) -> f32 {
    current + (target - current) * (EASE_RATE * dt).min(1.0)
}

fn spawn_glow(
    commands: &mut Commands,
    view: Entity,
    mesh: &Handle<Mesh>,
    material: Handle<GlowMaterial>,
) -> Entity {
    commands
        .spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            ChildOf(view),
        ))
        .id()
}

fn draw(
    glow: &Glow,
    color: Color,
    transform: Transform,
    parts: &mut Query<(
        &mut Transform,
        &mut Visibility,
        &MeshMaterial3d<GlowMaterial>,
    )>,
    materials: &mut Assets<GlowMaterial>,
) {
    let Ok((mut t, mut visibility, material)) = parts.get_mut(glow.entity) else {
        return;
    };
    let shown = glow.opacity > MIN_OPACITY;
    visibility.set_if_neq(if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !shown {
        return;
    }
    *t = transform;
    if let Some(mut m) = materials.get_mut(&material.0) {
        m.color = (color.to_linear() * glow.opacity).with_alpha(glow.opacity);
    }
}

pub(crate) fn sync_glow(
    mut commands: Commands,
    time: Res<Time>,
    meshes: Res<GlowMeshes>,
    mut materials: ResMut<Assets<GlowMaterial>>,
    mut views: Query<(Entity, &ViewProximity, &ViewPlayers, Option<&mut Glows>)>,
    mut parts: Query<(
        &mut Transform,
        &mut Visibility,
        &MeshMaterial3d<GlowMaterial>,
    )>,
) {
    let dt = time.delta_secs();
    for (view, proximity, players, glows) in &mut views {
        let mut fresh = Glows::default();
        let glows = match glows {
            Some(g) => g.into_inner(),
            None => &mut fresh,
        };
        let style = &proximity.style;
        // The phase in f64, so it stays smooth however long the view runs.
        let phase = (time.elapsed_secs_f64() * f64::from(style.pulse_speed)).fract() as f32;
        let pulse = 0.5 + 0.5 * (phase * TAU).sin();
        let flat = Quat::from_rotation_x(-FRAC_PI_2);
        let feet: HashMap<&str, Vec3> = players
            .players
            .iter()
            .map(|p| (p.id.as_str(), players.feet(p)))
            .chain(
                proximity
                    .undrawn
                    .iter()
                    .map(|(id, at)| (id.as_str(), Vec3::from(*at))),
            )
            .collect();

        glows.discs.retain(|id, glow| {
            let kept = feet.contains_key(id.as_str());
            if !kept {
                commands.entity(glow.entity).despawn();
            }
            kept
        });
        for (id, at) in &feet {
            let glow = glows.discs.entry(id.to_string()).or_insert_with(|| Glow {
                entity: spawn_glow(
                    &mut commands,
                    view,
                    &meshes.disc,
                    materials.add(GlowMaterial::default()),
                ),
                opacity: 0.0,
                radius: 0.0,
                at: None,
            });
            let state = proximity.players.get(*id).copied();
            // An area member's own disc gives way to its area's glow.
            let target = match state {
                Some(DiscState::Entering | DiscState::Exiting) => style.intensity * pulse,
                Some(DiscState::Entered) | None => 0.0,
            };
            glow.opacity = ease(glow.opacity, target, dt);
            let color = if state == Some(DiscState::Exiting) {
                style.exiting_color
            } else {
                style.entering_color
            };
            glow.at = Some(*at + Vec3::Y * LIFT);
            let transform = Transform::from_translation(*at + Vec3::Y * LIFT)
                .with_rotation(flat)
                .with_scale(Vec3::new(style.disc_radius, style.disc_radius, 1.0));
            draw(glow, color, transform, &mut parts, &mut materials);
        }

        glows.areas.retain(|id, glow| {
            let kept = proximity.areas.iter().any(|a| &a.id == id);
            if !kept {
                commands.entity(glow.entity).despawn();
            }
            kept
        });
        for area in &proximity.areas {
            let member_feet: Vec<f32> = area
                .members
                .iter()
                .filter_map(|m| feet.get(m.as_str()).map(|f| f.y))
                .collect();
            let glow = glows.areas.entry(area.id.clone()).or_insert_with(|| Glow {
                entity: spawn_glow(
                    &mut commands,
                    view,
                    &meshes.area,
                    materials.add(GlowMaterial::default()),
                ),
                opacity: 0.0,
                radius: area.radius,
                at: None,
            });
            // No member to stand it on: hidden until one comes back.
            if member_feet.is_empty() {
                glow.at = None;
                if let Ok((_, mut visibility, _)) = parts.get_mut(glow.entity) {
                    *visibility = Visibility::Hidden;
                }
                continue;
            }
            let y = member_feet.iter().sum::<f32>() / member_feet.len() as f32;
            glow.radius = ease(glow.radius, area.radius, dt);
            glow.opacity = ease(glow.opacity, style.intensity * AREA_INTENSITY, dt);
            let [x, z] = area.center;
            glow.at = Some(Vec3::new(x, y + LIFT, z));
            let transform = Transform::from_xyz(x, y + LIFT, z)
                .with_rotation(flat)
                .with_scale(Vec3::new(glow.radius, glow.radius, 1.0));
            draw(
                glow,
                style.entered_color,
                transform,
                &mut parts,
                &mut materials,
            );
        }

        if fresh.discs.len() + fresh.areas.len() > 0 {
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
    use crate::tests::bare_app;
    use crate::views::{ViewHub, ViewId, ViewKind, ViewMsg, ViewTarget};
    use crate::{PlayerRender, Players};

    fn app() -> (App, ViewId) {
        let mut app = bare_app();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            1.0 / 60.0,
        )));
        let hub = app.world().resource::<ViewHub>().clone();
        let id = hub.create(ViewKind::World, |_| ViewTarget::Image(UVec2::new(8, 8)));
        let players = ["a", "b", "c"]
            .iter()
            .enumerate()
            .map(|(i, id)| PlayerRender {
                id: id.to_string(),
                character: "Barbarian".into(),
                pos: Vec3::new(i as f32 * 2.0, 0.0, 0.0),
                yaw: 0.0,
                vel: Vec3::ZERO,
                name: String::new(),
                airborne: false,
                animation: None,
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
        for _ in 0..30 {
            app.update();
        }
    }

    /// The opacity of each shown glow, by its translation's x.
    fn shown(app: &mut App) -> Vec<(f32, f32)> {
        let mut q = app
            .world_mut()
            .query::<(&Transform, &Visibility, &MeshMaterial3d<GlowMaterial>)>();
        let materials = app.world().resource::<Assets<GlowMaterial>>();
        let mut shown: Vec<_> = q
            .iter(app.world())
            .filter(|(_, v, _)| **v != Visibility::Hidden)
            .map(|(t, _, m)| (t.translation.x, materials.get(&m.0).unwrap().color.alpha))
            .collect();
        shown.sort_by(|a, b| a.0.total_cmp(&b.0));
        shown
    }

    /// The colour of each shown glow, ordered by its translation's x.
    fn colors(app: &mut App) -> Vec<LinearRgba> {
        let mut q = app
            .world_mut()
            .query::<(&Transform, &Visibility, &MeshMaterial3d<GlowMaterial>)>();
        let materials = app.world().resource::<Assets<GlowMaterial>>();
        let mut shown: Vec<_> = q
            .iter(app.world())
            .filter(|(_, v, _)| **v != Visibility::Hidden)
            .map(|(t, _, m)| (t.translation.x, materials.get(&m.0).unwrap().color))
            .collect();
        shown.sort_by(|a, b| a.0.total_cmp(&b.0));
        shown.into_iter().map(|(_, c)| c).collect()
    }

    fn glows(app: &mut App) -> usize {
        let mut q = app.world_mut().query::<&MeshMaterial3d<GlowMaterial>>();
        q.iter(app.world()).count()
    }

    #[test]
    fn members_glow_as_their_area_and_an_approacher_as_its_disc() {
        let (mut app, id) = app();
        set(
            &mut app,
            id,
            json!({
                "players": {"a": "entered", "b": "entered", "c": "entering"},
                "areas": [{"id": "m", "center": [1, 0], "radius": 2.5, "members": ["a", "b"]}],
            }),
        );
        let shown = shown(&mut app);
        assert_eq!(shown.len(), 2, "{shown:?}");
        let (area_x, area_opacity) = shown[0];
        assert_eq!(area_x, 1.0);
        assert!(
            (area_opacity - AREA_INTENSITY).abs() < 0.05,
            "{area_opacity}"
        );
        assert_eq!(shown[1].0, 4.0);
        assert!(shown[1].1 > 0.0);
        assert_eq!(glows(&mut app), 4);
        let colors = colors(&mut app);
        let green = GlowStyle::default().entered_color.to_linear();
        let yellow = GlowStyle::default().entering_color.to_linear();
        assert!((colors[0].green / colors[0].red - green.green / green.red).abs() < 1e-3);
        assert!((colors[1].green / colors[1].red - yellow.green / yellow.red).abs() < 1e-3);
    }

    #[test]
    fn a_disc_fades_out_and_an_ended_area_is_removed() {
        let (mut app, id) = app();
        set(
            &mut app,
            id,
            json!({
                "players": {"a": "exiting"},
                "areas": [{"id": "m", "center": [1, 0], "radius": 2.5, "members": ["b", "c"]}],
            }),
        );
        assert_eq!(shown(&mut app).len(), 2);
        set(&mut app, id, json!({}));
        for _ in 0..120 {
            app.update();
        }
        assert!(shown(&mut app).is_empty());
        assert_eq!(glows(&mut app), 3, "one disc per player, no area");
    }

    #[test]
    fn an_undrawn_player_gets_a_disc_and_a_leaving_one_loses_it() {
        let (mut app, id) = app();
        set(
            &mut app,
            id,
            json!({"players": {"me": "entering"}, "undrawn": {"me": [9, 0, 0]}}),
        );
        assert_eq!(
            shown(&mut app).iter().map(|s| s.0).collect::<Vec<_>>(),
            [9.0]
        );
        assert_eq!(glows(&mut app), 4);

        let hub = app.world().resource::<ViewHub>().clone();
        hub.send(id, ViewMsg::Players(Players::default()));
        set(&mut app, id, json!({}));
        assert_eq!(glows(&mut app), 0);
    }
}
