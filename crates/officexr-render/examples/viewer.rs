//! Opens a window on a scene exported by `pnpm --filter @officexr/world
//! export:render-scene`.
//!
//! cargo run -p officexr-render --example viewer [scene.json] [asset base]
//!
//! The asset base is a directory or an `http(s)://` origin serving
//! `/models/...`; it defaults to `packages/studio/public` (populated by
//! `pnpm asset-packs:install`). With `OFFICEXR_SCREENSHOT=out.png` it saves
//! one frame once the models have had time to load, then exits.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use officexr_render::{AssetBase, OfficeXrRenderPlugin, SceneCamera, WorldSceneData};

fn main() {
    let mut args = std::env::args().skip(1);
    let scene_path = args.next().unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/booper.scene.json").into()
    });
    let base = args.next().unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/studio/public").into()
    });
    let base = if base.starts_with("http://") || base.starts_with("https://") {
        AssetBase::Http(base)
    } else {
        AssetBase::Path(base)
    };
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
        })
        .add_systems(Update, screenshot_and_exit)
        .run();
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
