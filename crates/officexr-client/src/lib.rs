//! Joins a live OfficeXR room and renders its world and moving peers.
//!
//! OFFICEXR_SESSION_COOKIE="<Cookie header>" cargo run -p officexr-client -- \
//!   --web-url http://localhost:5173 --realtime-url ws://localhost:8787 --office-id <uuid>
//!
//! Each flag falls back to an env var: `OFFICEXR_WEB_URL`, `OFFICEXR_REALTIME_URL`,
//! `OFFICEXR_OFFICE_ID`, `OFFICEXR_SESSION_COOKIE` (prefer it to `--cookie`, which `ps`
//! shows). WASD or the arrows walk, shift runs, space jumps, Alt+P cycles the camera; on a touch
//! screen the bottom-left stick walks and dragging elsewhere looks. On iOS the env vars come from
//! the Xcode scheme, with an in-app screen as the fallback (see `ios/README.md`). With
//! `OFFICEXR_SCREENSHOT=out.png` it saves one frame after `OFFICEXR_SCREENSHOT_FRAME` (default
//! 600) frames, then exits.
//!
//! The library half is [`OfficeRoomPlugin`] and [`connect`], for hosts that embed a room in
//! their own app; build it with `default-features = false` to leave out the winit app above.

mod baked_colliders;
mod broadcaster;
mod config;
#[cfg(all(feature = "standalone", target_os = "ios"))]
mod ios;
mod net;
mod peers;
mod player;
mod room;
mod world;

pub use config::Config;
pub use net::{Connection, Joined, RoomClosed, connect};
pub use peers::Peers;
pub use room::{OfficeRoomPlugin, asset_base, asset_plugin};

#[cfg(feature = "standalone")]
use bevy::prelude::*;
#[cfg(feature = "standalone")]
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
#[cfg(feature = "standalone")]
use officexr_controls::{CameraMode, ControlsHub, ControlsSystems};

#[cfg(all(feature = "standalone", target_os = "ios"))]
use ios::{connect_failed, load_config};

#[cfg(feature = "standalone")]
#[bevy_main]
pub fn main() {
    let config = load_config();
    let connection = connect(config.clone()).unwrap_or_else(|err| connect_failed(&config, err));

    let mut app = App::new();
    if let Some(screenshot) = ScreenshotAt::from_env() {
        app.insert_resource(screenshot);
    }
    app.add_plugins((
        asset_base(&config),
        DefaultPlugins.set(asset_plugin()),
        OfficeRoomPlugin::new(connection),
    ))
    .add_systems(
        Update,
        (exit_when_closed, cycle_camera_mode, screenshot_and_exit)
            .chain()
            .before(ControlsSystems),
    )
    .run();
}

#[cfg(feature = "standalone")]
fn exit_when_closed(mut closed: MessageReader<RoomClosed>, mut exit: MessageWriter<AppExit>) {
    if closed.read().next().is_some() {
        exit.write(AppExit::error());
    }
}

/// Alt+P cycles the gameplay camera modes, as on the web.
#[cfg(feature = "standalone")]
fn cycle_camera_mode(keys: Res<ButtonInput<KeyCode>>, hub: Res<ControlsHub>) {
    const MODES: [CameraMode; 3] = [
        CameraMode::FirstPerson,
        CameraMode::ThirdPerson,
        CameraMode::Fixed,
    ];
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    if alt && keys.just_pressed(KeyCode::KeyP) {
        hub.with(room::VIEW, |c| {
            let at = MODES.iter().position(|m| *m == c.mode).unwrap_or(0);
            c.mode = MODES[(at + 1) % MODES.len()];
        });
    }
}

#[cfg(all(feature = "standalone", not(target_os = "ios")))]
fn load_config() -> config::Config {
    config::Config::from_env_and_args().unwrap_or_else(|err| {
        eprintln!("{err}\n\n{}", config::USAGE);
        std::process::exit(2);
    })
}

#[cfg(all(feature = "standalone", not(target_os = "ios")))]
fn connect_failed(_: &config::Config, err: String) -> ! {
    eprintln!("{err}");
    std::process::exit(1);
}

#[cfg(feature = "standalone")]
#[derive(Resource)]
struct ScreenshotAt {
    path: String,
    frame: u32,
}

#[cfg(feature = "standalone")]
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

#[cfg(feature = "standalone")]
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
