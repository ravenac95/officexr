//! Runs a Bevy app on a surface someone else owns, without winit.
//!
//! Normally `bevy_winit` creates the window, inserts its `RawHandleWrapper`, and runs the
//! loop. Here the caller hands over the surface's handle and drives frames itself: [`run`]
//! loops on a thread of its own (Android), or the platform steps a [`Host`] from its display
//! callback (iOS).

use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::app::PluginsState;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::platform::sync::Mutex;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::{
    ExitCondition, PrimaryWindow, RawHandleWrapper, RawHandleWrapperHolder, WindowResolution,
};
use crossbeam_channel::{Receiver, Sender};
use officexr_client::{Config, OfficeRoomPlugin};
use serde::Deserialize;

use crate::events::{EventSender, HostEvent};
use crate::office::OfficeReportPlugin;
use crate::scene::{DemoScenePlugin, SceneCommand};

/// What the host shows, as Flutter sends it, e.g. `{"type":"demo"}` or
/// `{"type":"office","config":{"web_url":…,"realtime_url":…,"office_id":…,"session_cookie":…}}`.
#[derive(Deserialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SceneSpec {
    Demo,
    Office { config: Config },
}

impl SceneSpec {
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|err| format!("bad scene: {err}"))
    }
}

/// The surface to draw into, in physical pixels, and its logical-to-physical scale.
pub struct SurfaceTarget {
    pub handle: RawHandleWrapper,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

pub enum HostInput {
    /// A pointer event in logical pixels.
    Touch {
        id: u64,
        phase: TouchPhase,
        x: f32,
        y: f32,
    },
    Resize {
        width: u32,
        height: u32,
    },
    Scene(SceneCommand),
    Stop,
}

/// A Bevy app bound to one surface, advanced one frame at a time by its owner.
///
/// Create, step and drop it on one thread. On iOS that must be the main thread: wgpu only
/// creates a Metal surface from a `UIView` there, and Bevy creates surfaces on the thread
/// that calls `update`.
pub struct Host {
    app: App,
    window: Entity,
}

impl Host {
    /// Builds the app on `target` showing `scene`; blocks until the renderer is up, then
    /// reports `Ready`. An office first connects, which blocks on a login request; failing that,
    /// nothing is built and the error comes back.
    pub fn new(
        target: SurfaceTarget,
        scene: SceneSpec,
        events: Sender<HostEvent>,
    ) -> Result<Self, String> {
        let mut app = App::new();
        // The window entity must exist before `RenderPlugin::build`, which creates the wgpu
        // instance and picks an adapter compatible with the primary window's surface.
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: WindowResolution::new(target.width, target.height)
                        .with_scale_factor_override(target.scale),
                    ..default()
                },
                PrimaryWindow,
                target.handle.clone(),
                RawHandleWrapperHolder(Arc::new(Mutex::new(Some(target.handle)))),
            ))
            .id();
        let window_plugin = WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            close_when_requested: false,
            ..default()
        };
        match scene {
            SceneSpec::Demo => {
                app.add_plugins((DefaultPlugins.set(window_plugin), DemoScenePlugin));
            }
            SceneSpec::Office { config } => {
                // ponytail: connecting blocks the caller on the login request (iOS: the main
                // thread); fine for a prototype, an async connect would keep the UI responsive.
                let connection = officexr_client::connect(config.clone())?;
                app.add_plugins((
                    // Asset sources must exist before `DefaultPlugins` adds `AssetPlugin`.
                    officexr_client::asset_base(&config),
                    DefaultPlugins
                        .set(window_plugin)
                        .set(officexr_client::asset_plugin()),
                    OfficeRoomPlugin::new(connection),
                    OfficeReportPlugin,
                ));
            }
        }
        app.insert_resource(EventSender(events.clone()));

        // What `ScheduleRunnerPlugin` does before its first update: wait for async plugin
        // setup (the renderer's device request) to finish.
        while app.plugins_state() == PluginsState::Adding {
            bevy::tasks::tick_global_task_pools_on_main_thread();
        }
        app.finish();
        app.cleanup();

        let adapter = app.world().get_resource::<RenderAdapterInfo>().map_or_else(
            || "unknown".into(),
            |info| format!("{} ({:?})", info.name, info.backend),
        );
        let _ = events.send(HostEvent::Ready { adapter });
        Ok(Self { app, window })
    }

    /// Applies one input. `Stop` is the owner's to act on (by dropping the host), so it's
    /// ignored here.
    pub fn apply(&mut self, input: HostInput) {
        let world = self.app.world_mut();
        match input {
            HostInput::Touch { id, phase, x, y } => {
                world.write_message(TouchInput {
                    phase,
                    position: Vec2::new(x, y),
                    window: self.window,
                    force: None,
                    id,
                });
            }
            HostInput::Resize { width, height } => {
                if let Some(mut w) = world.get_mut::<Window>(self.window) {
                    w.resolution.set_physical_resolution(width, height);
                }
            }
            HostInput::Scene(command) => {
                world.write_message(command);
            }
            HostInput::Stop => {}
        }
    }

    /// Runs one frame: input systems, the scene, and rendering.
    pub fn frame(&mut self) {
        self.app.update();
    }
}

// ponytail: a fixed 60 fps cap; a real host would pace frames from Choreographer.
const FRAME: Duration = Duration::from_micros(16_667);

/// Runs a [`Host`] on the calling thread until [`HostInput::Stop`] or the input channel closes.
/// Android uses this from a thread of its own. If the host can't start, reports why and returns.
pub fn run(
    target: SurfaceTarget,
    scene: SceneSpec,
    inputs: Receiver<HostInput>,
    events: Sender<HostEvent>,
) {
    let mut host = match Host::new(target, scene, events.clone()) {
        Ok(host) => host,
        Err(message) => {
            let _ = events.send(HostEvent::Error { message });
            return;
        }
    };
    loop {
        let started = Instant::now();
        loop {
            match inputs.try_recv() {
                Ok(HostInput::Stop) | Err(crossbeam_channel::TryRecvError::Disconnected) => return,
                Ok(input) => host.apply(input),
                Err(crossbeam_channel::TryRecvError::Empty) => break,
            }
        }
        host.frame();
        if let Some(rest) = FRAME.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}
