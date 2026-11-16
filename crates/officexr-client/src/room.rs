//! A live office room as a Bevy plugin: the world, the other people in it, and the local player
//! walking it with the gameplay camera. Both the standalone client and embedding hosts (the
//! Flutter prototype in `mobile-flutter/`) add it.

use std::sync::Mutex;

use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use officexr_controls::{CameraMode, ControlsHub, ControlsPlugin, ControlsSystems};
use officexr_render::{AssetBase, OfficeXrRenderPlugin, Players, SceneCamera};

use crate::config::Config;
use crate::net::{self, Connection};
use crate::{baked_colliders, peers, player, world};

/// The only view the room draws into.
pub(crate) const VIEW: u32 = 0;

/// What view 0 shows until the player spawns: the old orbit's eye, looking at the origin.
const LOADING_CAMERA: SceneCamera = SceneCamera {
    position: [0.0, 3.8, 7.0],
    look_at: [0.0; 3],
    fov_deg: 60.0,
    near: 0.1,
    far: 2000.0,
};

/// Runs a room over a [`Connection`]. Add [`asset_base`] before `DefaultPlugins` (configured
/// with [`asset_plugin`]), then this.
pub struct OfficeRoomPlugin {
    // `Plugin::build` only borrows the plugin; the connection moves into the app once.
    connection: Mutex<Option<Connection>>,
}

impl OfficeRoomPlugin {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection: Mutex::new(Some(connection)),
        }
    }
}

impl Plugin for OfficeRoomPlugin {
    fn build(&self, app: &mut App) {
        let Connection { inbound, outbound } = self
            .connection
            .lock()
            .unwrap()
            .take()
            .expect("OfficeRoomPlugin is added once");
        app.add_plugins((OfficeXrRenderPlugin, ControlsPlugin))
            .insert_resource(LOADING_CAMERA)
            .insert_resource(inbound)
            .insert_resource(outbound)
            .init_resource::<player::LocalPlayer>()
            .init_resource::<Players>()
            .init_resource::<peers::Peers>()
            .add_message::<world::WorldUpdate>()
            .add_message::<peers::PeerUpdate>()
            .add_message::<net::Joined>()
            .add_message::<net::RoomClosed>()
            .add_systems(Startup, third_person)
            .add_systems(
                Update,
                (
                    net::receive_inbound,
                    world::assemble_world,
                    peers::update_peers,
                    baked_colliders::load_baked_colliders,
                    player::apply_colliders,
                    player::step_player,
                )
                    .chain()
                    .before(ControlsSystems),
            );
    }
}

/// Where the room's models load from: the web server named in `config`.
pub fn asset_base(config: &Config) -> AssetBase {
    AssetBase::parse(config.web_url.clone())
}

/// The `AssetPlugin` a room needs in `DefaultPlugins`.
pub fn asset_plugin() -> AssetPlugin {
    AssetPlugin {
        // The web server answers unknown paths with its SPA page, not a 404.
        meta_check: AssetMetaCheck::Never,
        ..default()
    }
}

fn third_person(hub: Res<ControlsHub>) {
    hub.with(VIEW, |c| c.mode = CameraMode::ThirdPerson);
}
