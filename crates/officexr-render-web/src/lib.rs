//! `officexr-render` in a browser canvas, fed from JS: `start`, then
//! `setScene` whenever the map changes and `setPlayers` every frame.
#![cfg(target_arch = "wasm32")]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;
use officexr_render::{
    AssetBase, AvatarBound, LayoutIndex, ObjectKinds, OfficeXrRenderPlugin, PlayerRender, Players,
    SceneCamera, WorldSceneData, mugshot_player,
};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

/// Frames drawn after every model has loaded before `isReady`, so the
/// swapped-in avatar materials have rendered.
const SETTLE_FRAMES: u32 = 3;

static INBOX: Mutex<Inbox> = Mutex::new(Inbox {
    scene: None,
    players: None,
});
static READY: AtomicBool = AtomicBool::new(false);

struct Inbox {
    scene: Option<(WorldSceneData, SceneCamera)>,
    players: Option<Players>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CameraInput {
    position: [f32; 3],
    look_at: [f32; 3],
    fov_deg: f32,
    near: f32,
    far: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlayersInput {
    players: Vec<PlayerInput>,
    char_radius: f32,
    player_speed: f32,
    run_speed: f32,
}

#[derive(Deserialize)]
struct PlayerInput {
    id: String,
    character: String,
    pos: [f32; 3],
    yaw: f32,
    vel: [f32; 3],
    name: String,
    airborne: bool,
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// Boots Bevy on the `canvas` selector; `origin` serves `/models/...`.
#[wasm_bindgen]
pub fn start(canvas: String, origin: String) {
    App::new()
        .add_plugins((
            AssetBase::Http(origin),
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        canvas: Some(canvas),
                        fit_canvas_to_parent: true,
                        ..default()
                    }),
                    ..default()
                })
                // The dev server answers missing `.meta` files with index.html.
                .set(AssetPlugin {
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                }),
            OfficeXrRenderPlugin,
        ))
        .init_resource::<Players>()
        .add_systems(PreUpdate, drain_inbox)
        .add_systems(Last, track_ready)
        .run();
}

/// Replaces the scene (`WorldSceneData` JSON). With `mugshot`, that
/// character is settled on the scene and drawn paused instead of the
/// `setPlayers` feed.
#[wasm_bindgen(js_name = setScene)]
pub fn set_scene(
    scene_json: &str,
    camera: JsValue,
    mugshot: Option<String>,
) -> Result<(), JsError> {
    let scene: WorldSceneData = serde_json::from_str(scene_json).map_err(js_err)?;
    let camera: CameraInput = serde_wasm_bindgen::from_value(camera).map_err(js_err)?;
    let mut inbox = INBOX.lock().unwrap();
    inbox.players = mugshot.map(|character| mugshot_player(scene_json, character));
    inbox.scene = Some((
        scene,
        SceneCamera {
            position: camera.position,
            look_at: camera.look_at,
            fov_deg: camera.fov_deg,
            near: camera.near,
            far: camera.far,
        },
    ));
    READY.store(false, Ordering::Relaxed);
    Ok(())
}

#[wasm_bindgen(js_name = setPlayers)]
pub fn set_players(players: JsValue) -> Result<(), JsError> {
    let input: PlayersInput = serde_wasm_bindgen::from_value(players).map_err(js_err)?;
    INBOX.lock().unwrap().players = Some(Players {
        players: input
            .players
            .into_iter()
            .map(|p| PlayerRender {
                id: p.id,
                character: p.character,
                pos: p.pos.into(),
                yaw: p.yaw,
                vel: p.vel.into(),
                name: p.name,
                airborne: p.airborne,
            })
            .collect(),
        char_radius: input.char_radius,
        player_speed: input.player_speed,
        run_speed: input.run_speed,
        ..default()
    });
    Ok(())
}

/// Every model of the last scene has loaded and settled.
#[wasm_bindgen(js_name = isReady)]
pub fn is_ready() -> bool {
    READY.load(Ordering::Relaxed)
}

fn drain_inbox(mut commands: Commands, mut players: ResMut<Players>) {
    let mut inbox = INBOX.lock().unwrap();
    if let Some((scene, camera)) = inbox.scene.take() {
        commands.insert_resource(scene);
        commands.insert_resource(camera);
    }
    if let Some(next) = inbox.players.take() {
        // A mugshot's paused player stays put until the next scene.
        if !players.paused || next.paused {
            *players = next;
        }
    }
}

fn track_ready(
    mut settled: Local<u32>,
    scene: Option<Res<WorldSceneData>>,
    kinds: Res<ObjectKinds>,
    layouts: Res<LayoutIndex>,
    players: Res<Players>,
    bound: Query<(), With<AvatarBound>>,
) {
    let loaded = scene.is_some_and(|s| !s.is_changed())
        && !kinds.is_loading()
        && !layouts.is_loading()
        && bound.iter().len() == players.players.len();
    *settled = if loaded { *settled + 1 } else { 0 };
    if *settled == SETTLE_FRAMES {
        READY.store(true, Ordering::Relaxed);
    }
}
