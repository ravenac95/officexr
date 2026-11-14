use bevy::prelude::*;
use officexr_sim::physics::character_movement::XZ;
use officexr_sim::physics::rules::{BODY_Y, SPAWN_DROP_HEIGHT};
use officexr_sim::{SimCore, SimInput, SimKind, SimKinds, SimSettings, WorldObjects};

use crate::{PlayerRender, Players};

/// The scene's kinds and objects, read as the sim's collider inputs.
#[derive(serde::Deserialize)]
struct SimScene {
    kinds: Vec<SimKind>,
    objects: WorldObjects,
}

/// A sim colliding with the scene JSON's kinds and objects.
pub fn scene_sim(scene_json: &str, char_radius: f32) -> SimCore {
    let scene: SimScene = serde_json::from_str(scene_json).expect("scene colliders");
    let mut sim = SimCore::new(char_radius.into());
    let kinds = SimKinds {
        voxel_size: scene.objects.cube_size,
        kinds: scene.kinds,
    };
    sim.set_static_colliders(&scene.objects, &kinds, &[])
        .expect("valid colliders");
    sim
}

/// One 60 Hz step walking `dir` (zero stands still) at `speed`, facing `yaw`.
pub fn sim_input(players: &Players, dir: XZ, speed: f32, yaw: f32) -> SimInput {
    SimInput {
        dt_sec: 1.0 / 60.0,
        now_ms: 0.0,
        dir,
        speed: speed.into(),
        running: false,
        yaw: yaw.into(),
        jump: false,
        settings: SimSettings {
            char_radius: players.char_radius.into(),
            movement_block_threshold: 0.9,
            jump_velocity: 8.0,
            air_control: 0.2,
            max_jumps: 2,
            landing_ease_ms: 120.0,
        },
        spawn_points: vec![],
    }
}

/// `character` dropped from `SPAWN_DROP_HEIGHT` above the origin and
/// settled on the scene's colliders, paused, as the studio's Mugshot mode
/// draws it.
pub fn mugshot_player(scene_json: &str, character: String) -> Players {
    let players = Players {
        paused: true,
        body_y: BODY_Y as f32,
        ..default()
    };
    let mut sim = scene_sim(scene_json, players.char_radius);
    sim.teleport(officexr_sim::Vec3 {
        x: 0.0,
        y: SPAWN_DROP_HEIGHT,
        z: 0.0,
    });
    let input = sim_input(&players, XZ { x: 0.0, z: 0.0 }, 0.0, 0.0);
    let pos = (0..600)
        .map(|_| sim.step(&input))
        .find(|r| r.is_grounded)
        .expect("the character never landed")
        .new_pos;
    Players {
        players: vec![PlayerRender {
            id: "mugshot-player".into(),
            character,
            pos: Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32),
            yaw: 0.0,
            vel: Vec3::ZERO,
            name: String::new(),
            airborne: false,
        }],
        ..players
    }
}
