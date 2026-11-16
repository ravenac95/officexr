//! Runs a Bevy app on a surface someone else owns, without winit.
//!
//! Normally `bevy_winit` creates the window, inserts its `RawHandleWrapper`, and runs the
//! loop. Here the caller hands over the surface's handle, and [`run`] drives `app.update()`
//! itself on whatever thread it's called on, feeding in touches and commands between frames.

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

use crate::scene::{DemoScenePlugin, EventSender, HostEvent, SceneCommand};

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

// ponytail: a fixed 60 fps cap; a real host would pace frames from Choreographer.
const FRAME: Duration = Duration::from_micros(16_667);

/// Builds the app on `target` and runs it until [`HostInput::Stop`] or the input channel closes.
pub fn run(target: SurfaceTarget, inputs: Receiver<HostInput>, events: Sender<HostEvent>) {
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
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            close_when_requested: false,
            ..default()
        }),
        DemoScenePlugin,
    ))
    .insert_resource(EventSender(events.clone()));

    // What `ScheduleRunnerPlugin` does before its first update: wait for async plugin setup
    // (the renderer's device request) to finish.
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();

    let adapter = app
        .world()
        .get_resource::<RenderAdapterInfo>()
        .map_or_else(|| "unknown".into(), |info| format!("{} ({:?})", info.name, info.backend));
    let _ = events.send(HostEvent::Ready { adapter });

    loop {
        let started = Instant::now();
        loop {
            let input = match inputs.try_recv() {
                Ok(input) => input,
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => return,
            };
            match input {
                HostInput::Touch { id, phase, x, y } => {
                    app.world_mut().write_message(TouchInput {
                        phase,
                        position: Vec2::new(x, y),
                        window,
                        force: None,
                        id,
                    });
                }
                HostInput::Resize { width, height } => {
                    if let Some(mut w) = app.world_mut().get_mut::<Window>(window) {
                        w.resolution.set_physical_resolution(width, height);
                    }
                }
                HostInput::Scene(command) => {
                    app.world_mut().write_message(command);
                }
                HostInput::Stop => return,
            }
        }
        app.update();
        if let Some(rest) = FRAME.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
}

