//! Joins a live OfficeXR room and renders its world and moving peers.
//!
//! OFFICEXR_SESSION_COOKIE="<Cookie header>" cargo run -p officexr-client -- \
//!   --web-url http://localhost:5173 --realtime-url ws://localhost:8787 --office-id <uuid>
//!
//! Each flag falls back to an env var: `OFFICEXR_WEB_URL`, `OFFICEXR_REALTIME_URL`,
//! `OFFICEXR_OFFICE_ID`, `OFFICEXR_SESSION_COOKIE` (prefer it to `--cookie`, which `ps`
//! shows). WASD or the arrows walk, shift runs, space jumps, Alt+P cycles the camera; on a touch
//! screen the bottom-left stick walks and dragging elsewhere looks. On iOS the env vars come from the Xcode scheme, with
//! an in-app screen as the fallback (see `ios/README.md`). With `OFFICEXR_SCREENSHOT=out.png` it
//! saves one frame after `OFFICEXR_SCREENSHOT_FRAME` (default 600) frames, then exits.

mod baked_colliders;
mod broadcaster;
mod config;
#[cfg(target_os = "ios")]
mod ios;
mod net;
mod peers;
mod player;
mod touch;
mod world;

use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use officexr_controls::{CameraMode, ControlsHub, ControlsPlugin, ControlsSystems};
use officexr_render::{AssetBase, OfficeXrRenderPlugin, Players, SceneCamera};

#[cfg(target_os = "ios")]
use ios::{connect_failed, load_config};

/// The only view the client draws into.
const VIEW: u32 = 0;

/// What view 0 shows until the player spawns: the old orbit's eye, looking at the origin.
const LOADING_CAMERA: SceneCamera = SceneCamera {
    position: [0.0, 3.8, 7.0],
    look_at: [0.0; 3],
    fov_deg: 60.0,
    near: 0.1,
    far: 2000.0,
};

#[bevy_main]
pub fn main() {
    let config = load_config();
    let base = AssetBase::parse(config.web_url.clone());
    let (inbound, outbound) =
        net::spawn(config.clone()).unwrap_or_else(|err| connect_failed(&config, err));

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
        ControlsPlugin,
    ))
    .insert_resource(LOADING_CAMERA)
    .insert_resource(inbound)
    .insert_resource(outbound)
    .init_resource::<player::LocalPlayer>()
    .init_resource::<touch::TouchControls>()
    .init_resource::<Players>()
    .init_resource::<peers::Peers>()
    .add_message::<world::WorldUpdate>()
    .add_message::<peers::PeerUpdate>()
    .add_message::<net::Joined>()
    .add_systems(Startup, (touch::spawn_stick, third_person))
    .add_systems(
        Update,
        (
            net::receive_inbound,
            world::assemble_world,
            peers::update_peers,
            baked_colliders::load_baked_colliders,
            player::apply_colliders,
            touch::read_touches,
            player::step_player,
            cycle_camera_mode,
            screenshot_and_exit,
        )
            .chain()
            .before(ControlsSystems),
    )
    .run();
}

fn third_person(hub: Res<ControlsHub>) {
    hub.with(VIEW, |c| c.mode = CameraMode::ThirdPerson);
}

/// Alt+P cycles the gameplay camera modes, as on the web.
fn cycle_camera_mode(keys: Res<ButtonInput<KeyCode>>, hub: Res<ControlsHub>) {
    const MODES: [CameraMode; 3] = [
        CameraMode::FirstPerson,
        CameraMode::ThirdPerson,
        CameraMode::Fixed,
    ];
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    if alt && keys.just_pressed(KeyCode::KeyP) {
        hub.with(VIEW, |c| {
            let at = MODES.iter().position(|m| *m == c.mode).unwrap_or(0);
            c.mode = MODES[(at + 1) % MODES.len()];
        });
    }
}

#[cfg(not(target_os = "ios"))]
fn load_config() -> config::Config {
    config::Config::from_env_and_args().unwrap_or_else(|err| {
        eprintln!("{err}\n\n{}", config::USAGE);
        std::process::exit(2);
    })
}

#[cfg(not(target_os = "ios"))]
fn connect_failed(_: &config::Config, err: String) -> ! {
    eprintln!("{err}");
    std::process::exit(1);
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
