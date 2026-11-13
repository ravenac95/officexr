//! Joins a live OfficeXR room and renders its world and moving peers.
//!
//! OFFICEXR_SESSION_COOKIE="<Cookie header>" cargo run -p officexr-client -- \
//!   --web-url http://localhost:5173 --realtime-url ws://localhost:8787 --office-id <uuid>
//!
//! Each flag falls back to an env var: `OFFICEXR_WEB_URL`, `OFFICEXR_REALTIME_URL`,
//! `OFFICEXR_OFFICE_ID`, `OFFICEXR_SESSION_COOKIE` (prefer it to `--cookie`, which `ps`
//! shows). WASD or the arrows walk, shift runs, space jumps. With `OFFICEXR_SCREENSHOT=out.png` it saves one frame after `OFFICEXR_SCREENSHOT_FRAME` (default 600) frames, then exits.

mod baked_colliders;
mod broadcaster;
mod camera;
mod config;
mod net;
mod peers;
mod player;
mod world;

use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use officexr_render::{AssetBase, OfficeXrRenderPlugin, Players};

fn main() {
    let config = config::Config::from_env_and_args().unwrap_or_else(|err| {
        eprintln!("{err}\n\n{}", config::USAGE);
        std::process::exit(2);
    });
    let base = AssetBase::parse(config.web_url.clone());
    let (inbound, outbound) = net::spawn(config).unwrap_or_else(|err| {
        eprintln!("{err}");
        std::process::exit(1);
    });

    let mut app = App::new();
    if let Some(screenshot) = ScreenshotAt::from_env() {
        app.insert_resource(screenshot);
    }
    app.add_plugins((
        base,
        // The web server answers unknown paths with its SPA page, not a 404.
        DefaultPlugins.set(AssetPlugin {
            meta_check: AssetMetaCheck::Never,
            ..default()
        }),
        OfficeXrRenderPlugin,
    ))
    .insert_resource(inbound)
    .insert_resource(outbound)
    .init_resource::<player::LocalPlayer>()
    .insert_resource(camera::initial())
    .init_resource::<camera::Orbit>()
    .init_resource::<Players>()
    .init_resource::<peers::Peers>()
    .add_message::<world::WorldUpdate>()
    .add_message::<peers::PeerUpdate>()
    .add_message::<net::Joined>()
    .add_systems(
        Update,
        (
            net::receive_inbound,
            world::assemble_world,
            peers::update_peers,
            baked_colliders::load_baked_colliders,
            player::apply_colliders,
            player::step_player,
            camera::orbit_camera,
            screenshot_and_exit,
        )
            .chain(),
    )
    .run();
}

#[derive(Resource)]
struct ScreenshotAt {
    path: String,
    frame: u32,
}

impl ScreenshotAt {
    fn from_env() -> Option<Self> {
        Some(Self {
            path: std::env::var("OFFICEXR_SCREENSHOT").ok()?,
            frame: std::env::var("OFFICEXR_SCREENSHOT_FRAME")
                .ok()
                .and_then(|f| f.parse().ok())
                .unwrap_or(600),
        })
    }
}

fn screenshot_and_exit(
    mut commands: Commands,
    screenshot: Option<Res<ScreenshotAt>>,
    mut frame: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(screenshot) = screenshot else {
        return;
    };
    let (path, at) = (screenshot.path.clone(), screenshot.frame);
    *frame += 1;
    if *frame == at {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    } else if *frame == at + 20 {
        exit.write(AppExit::Success);
    }
}
