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

/// `character` dropped from `SPAWN_DROP_HEIGHT` above the origin and
/// settled on the scene's colliders, paused, as the studio's Mugshot mode
/// draws it.
pub fn mugshot_player(scene_json: &str, character: String) -> Players {
    let scene: SimScene = serde_json::from_str(scene_json).expect("scene colliders");
    let players = Players {
        paused: true,
        body_y: BODY_Y as f32,
        ..default()
    };
    let settings = SimSettings {
        char_radius: players.char_radius.into(),
        movement_block_threshold: 0.9,
        jump_velocity: 8.0,
        air_control: 0.2,
        max_jumps: 2,
        landing_ease_ms: 120.0,
    };
    let mut sim = SimCore::new(settings.char_radius);
    let kinds = SimKinds {
        voxel_size: scene.objects.cube_size,
        kinds: scene.kinds,
    };
    sim.set_static_colliders(&scene.objects, &kinds, &[])
        .expect("valid colliders");
    sim.teleport(officexr_sim::Vec3 {
        x: 0.0,
        y: SPAWN_DROP_HEIGHT,
        z: 0.0,
    });
    let input = SimInput {
        dt_sec: 1.0 / 60.0,
        now_ms: 0.0,
        dir: XZ { x: 0.0, z: 0.0 },
        speed: 0.0,
        running: false,
        yaw: 0.0,
        jump: false,
        settings,
        spawn_points: vec![],
    };
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
