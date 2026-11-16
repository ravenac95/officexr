//! What the host tells Flutter, whichever scene it runs.

use bevy::prelude::*;
use crossbeam_channel::Sender;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostEvent {
    /// The renderer is up; `adapter` names the GPU and backend wgpu picked.
    Ready {
        adapter: String,
    },
    /// The demo scene's frame rate and cube count.
    Stats {
        fps: f32,
        cubes: usize,
    },
    /// An office room's frame rate, whether it has joined, and how many others are in it.
    Room {
        fps: f32,
        joined: bool,
        peers: usize,
    },
    /// The office's realtime connection gave up.
    Closed {
        reason: String,
    },
    Error {
        message: String,
    },
}

#[derive(Resource, Clone)]
pub struct EventSender(pub Sender<HostEvent>);

impl EventSender {
    pub fn send(&self, event: HostEvent) {
        let _ = self.0.send(event);
    }
}

/// Counts frames and reports the rate every half second.
#[derive(Default)]
pub struct FpsCounter {
    frames: u32,
    elapsed: f32,
}

impl FpsCounter {
    /// Adds a frame of `delta` seconds; returns the rate once half a second has passed.
    pub fn tick(&mut self, delta: f32) -> Option<f32> {
        self.frames += 1;
        self.elapsed += delta;
        if self.elapsed < 0.5 {
            return None;
        }
        let fps = self.frames as f32 / self.elapsed;
        *self = Self::default();
        Some(fps)
    }
}
