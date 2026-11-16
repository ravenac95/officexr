//! The demo scene: spinning cubes on a floor under an orbit camera. It needs no server, so CI's
//! smoke tests run it.
//!
//! It knows nothing about Android or Flutter. The host feeds it [`SceneCommand`]s and
//! `TouchInput` messages, and it reports [`HostEvent`]s back over a channel.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::input::touch::Touches;
use bevy::prelude::*;
use bevy::render::view::Tonemapping;
use serde::Deserialize;

use crate::events::{EventSender, FpsCounter, HostEvent};

/// What Flutter can ask the scene to do.
#[derive(Message, Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SceneCommand {
    /// Spin speed in turns per second.
    SetSpeed {
        turns_per_second: f32,
    },
    /// The cubes' base colour, sRGB 0..1.
    SetColor {
        r: f32,
        g: f32,
        b: f32,
    },
    AddCube,
    RemoveCube,
    ResetCamera,
}

pub struct DemoScenePlugin;

impl Plugin for DemoScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SceneCommand>()
            .insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.12)))
            .init_resource::<Orbit>()
            .init_resource::<CubeStyle>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    apply_commands,
                    orbit_from_touches,
                    place_camera,
                    spin,
                    report_stats,
                )
                    .chain(),
            );
    }
}

#[derive(Resource)]
struct Orbit {
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: 0.6,
            pitch: 0.45,
            distance: 9.0,
        }
    }
}

#[derive(Resource)]
struct CubeStyle {
    material: Handle<StandardMaterial>,
    mesh: Handle<Mesh>,
    turns_per_second: f32,
}

impl FromWorld for CubeStyle {
    fn from_world(world: &mut World) -> Self {
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(0.9, 0.9, 0.9));
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::srgb(0.25, 0.55, 1.0),
                perceptual_roughness: 0.4,
                ..default()
            });
        Self {
            material,
            mesh,
            turns_per_second: 0.25,
        }
    }
}

#[derive(Component)]
struct Spinner {
    phase: f32,
}

const START_CUBES: usize = 6;
const MAX_CUBES: usize = 64;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    style: Res<CubeStyle>,
) {
    // Tony McMapface (the default) needs the LUT feature, which this build leaves out.
    commands.spawn((Camera3d::default(), Tonemapping::AcesFitted));
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 300.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(14.0, 14.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.32, 0.34, 0.38))),
    ));
    for i in 0..START_CUBES {
        spawn_cube(&mut commands, &style, i);
    }
}

/// Places the `index`th cube on a ring, adding rings as it fills.
fn spawn_cube(commands: &mut Commands, style: &CubeStyle, index: usize) {
    let ring = index / 8;
    let slot = index % 8;
    let radius = 2.2 + ring as f32 * 1.6;
    let angle = slot as f32 / 8.0 * TAU + ring as f32 * 0.4;
    commands.spawn((
        Mesh3d(style.mesh.clone()),
        MeshMaterial3d(style.material.clone()),
        Transform::from_xyz(radius * angle.cos(), 0.75, radius * angle.sin()),
        Spinner {
            phase: index as f32 * 0.7,
        },
    ));
}

fn apply_commands(
    mut commands: Commands,
    mut incoming: MessageReader<SceneCommand>,
    mut style: ResMut<CubeStyle>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut orbit: ResMut<Orbit>,
    cubes: Query<Entity, With<Spinner>>,
) {
    let mut count = cubes.iter().count();
    for command in incoming.read() {
        match command {
            SceneCommand::SetSpeed { turns_per_second } => {
                style.turns_per_second = turns_per_second.clamp(-4.0, 4.0);
            }
            SceneCommand::SetColor { r, g, b } => {
                if let Some(mut material) = materials.get_mut(&style.material) {
                    material.base_color = Color::srgb(*r, *g, *b);
                }
            }
            SceneCommand::AddCube => {
                if count < MAX_CUBES {
                    spawn_cube(&mut commands, &style, count);
                    count += 1;
                }
            }
            SceneCommand::RemoveCube => {
                // Entity order isn't spawn order, but any cube going is fine for a demo.
                if let Some(entity) = cubes.iter().last() {
                    commands.entity(entity).despawn();
                    count = count.saturating_sub(1);
                }
            }
            SceneCommand::ResetCamera => *orbit = Orbit::default(),
        }
    }
}

/// One finger orbits; two fingers pinch to zoom.
fn orbit_from_touches(touches: Res<Touches>, mut orbit: ResMut<Orbit>) {
    let pressed: Vec<_> = touches.iter().collect();
    match pressed.as_slice() {
        [one] => {
            let delta = one.delta();
            orbit.yaw -= delta.x * 0.008;
            orbit.pitch = (orbit.pitch + delta.y * 0.006).clamp(0.05, FRAC_PI_2 - 0.05);
        }
        [a, b] => {
            let now = a.position().distance(b.position());
            let before = (a.position() - a.delta()).distance(b.position() - b.delta());
            if now > 1.0 && before > 1.0 {
                orbit.distance = (orbit.distance * before / now).clamp(3.0, 30.0);
            }
        }
        _ => {}
    }
}

fn place_camera(orbit: Res<Orbit>, mut camera: Query<&mut Transform, With<Camera3d>>) {
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let offset = Vec3::new(
        orbit.pitch.cos() * orbit.yaw.sin(),
        orbit.pitch.sin(),
        orbit.pitch.cos() * orbit.yaw.cos(),
    ) * orbit.distance;
    *transform = Transform::from_translation(offset).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y);
}

fn spin(time: Res<Time>, style: Res<CubeStyle>, mut cubes: Query<(&mut Transform, &Spinner)>) {
    let t = time.elapsed_secs();
    for (mut transform, spinner) in &mut cubes {
        let angle = (t * style.turns_per_second) * TAU + spinner.phase;
        transform.rotation = Quat::from_euler(EulerRot::YXZ, angle, angle * 0.5, 0.0);
        transform.translation.y = 0.75 + 0.25 * (t * 2.0 + spinner.phase).sin();
    }
}

fn report_stats(
    time: Res<Time>,
    mut counter: Local<FpsCounter>,
    cubes: Query<(), With<Spinner>>,
    events: Option<Res<EventSender>>,
) {
    let (Some(fps), Some(events)) = (counter.tick(time.delta_secs()), events) else {
        return;
    };
    events.send(HostEvent::Stats {
        fps,
        cubes: cubes.iter().count(),
    });
}
