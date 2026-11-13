//! Opens a window on a scene exported by `pnpm --filter @officexr/world
//! export:render-scene`. To view a map's baked layouts:
//!
//! pnpm --filter @officexr/world bake:layout --map default
//! pnpm --filter @officexr/world export:render-scene default /tmp/default.scene.json
//! cargo run -p officexr-render --example viewer /tmp/default.scene.json
//!
//! The asset base is a directory or an `http(s)://` origin serving
//! `/models/...`; it defaults to `packages/studio/public` (populated by
//! `pnpm asset-packs:install`). With `OFFICEXR_SCREENSHOT=out.png` it saves
//! one frame once the models have had time to load, then exits.
//!
//! The six KayKit characters stand in a row in front of the camera, each
//! cycling idle, walk, run and jump on the spot.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use officexr_render::{
    AssetBase, OfficeXrRenderPlugin, PlayerRender, Players, SceneCamera, WorldSceneData,
};

const CHARACTERS: [&str; 6] = [
    "Barbarian",
    "Knight",
    "Mage",
    "Ranger",
    "Rogue",
    "Rogue_Hooded",
];
const SECONDS_PER_MOTION: f32 = 3.0;

fn main() {
    let mut args = std::env::args().skip(1);
    let scene_path = args.next().unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/booper.scene.json").into()
    });
    let base = args.next().unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/studio/public").into()
    });
    let base = AssetBase::parse(base);
    let scene: WorldSceneData = serde_json::from_str(
        &std::fs::read_to_string(&scene_path).unwrap_or_else(|e| panic!("{scene_path}: {e}")),
    )
    .unwrap_or_else(|e| panic!("{scene_path}: {e}"));

    App::new()
        .add_plugins((base, DefaultPlugins, OfficeXrRenderPlugin))
        .insert_resource(scene)
        .insert_resource(SceneCamera {
            position: [1.0, 16.0, 26.0],
            look_at: [1.0, 0.0, 2.0],
            fov_deg: 60.0,
            near: 0.1,
            far: 2000.0,
        })
        .init_resource::<Players>()
        .add_systems(Update, (cycle_motions, screenshot_and_exit))
        .run();
}

fn cycle_motions(time: Res<Time>, mut players: ResMut<Players>) {
    let step = (time.elapsed_secs() / SECONDS_PER_MOTION) as usize;
    let speeds = [0.0, players.player_speed, players.run_speed, 0.0];
    players.players = CHARACTERS
        .iter()
        .enumerate()
        .map(|(i, character)| {
            let tier = (step + i) % 4;
            PlayerRender {
                id: character.to_string(),
                character: character.to_string(),
                // Standing on the booper floor's cube tops at y=2.
                pos: Vec3::new(i as f32 * 1.5 - 1.0, 1.5, 6.0),
                // Facing the camera.
                yaw: std::f32::consts::PI,
                vel: Vec3::new(speeds[tier], 0.0, 0.0),
                name: character.to_string(),
                airborne: tier == 3,
            }
        })
        .collect();
}

fn screenshot_and_exit(
    mut commands: Commands,
    mut frame: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(path) = std::env::var("OFFICEXR_SCREENSHOT") else {
        return;
    };
    *frame += 1;
    if *frame == 300 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    } else if *frame == 320 {
        exit.write(AppExit::Success);
    }
}
