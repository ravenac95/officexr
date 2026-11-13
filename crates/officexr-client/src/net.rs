use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use officexr_protocol::{
    Incoming, NetEvent, NetEventVariant0Kind, NetEventVariant3Kind, NetEventVariant10Kind,
    NetEventVariant11Kind, NetEventVariant14Kind, WsClient,
};
use officexr_render::BakedLayoutPlacement;
use officexr_sim::Vec3 as SimVec3;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc as tokio_mpsc;

use crate::broadcaster::Pose;
use crate::config::Config;
use crate::peers::{PeerUpdate, Position};
use crate::world::{Layout, WorldUpdate};

enum Inbound {
    Kinds(Vec<Value>),
    Map(Vec<Layout>, Vec<SimVec3>),
    Net(Incoming),
    /// The realtime client gave up; its message says why.
    Closed(String),
}

#[derive(Resource)]
pub struct InboundRx {
    rx: Mutex<mpsc::Receiver<Inbound>>,
    self_id: String,
}

/// (Re)joined the room.
#[derive(Message)]
pub struct Joined;

/// Sends the local player's events, numbered from the same `seq` as the client's own.
#[derive(Resource)]
pub struct Outbound {
    tx: tokio_mpsc::UnboundedSender<NetEvent>,
    seq: Arc<AtomicU64>,
    pub self_id: String,
}

impl Outbound {
    pub fn send_position(&self, pose: Pose) {
        let v = |v: SimVec3| json!({ "x": v.x, "y": v.y, "z": v.z });
        let event = serde_json::from_value(json!({
            "kind": "presence:position", "v": 1, "actorId": self.self_id,
            "seq": take_seq(&self.seq), "t": now_ms(),
            "pos": v(pose.pos), "vel": v(pose.vel), "yaw": pose.yaw, "isAirborne": pose.is_airborne,
        }))
        .expect("presence:position matches the schema");
        let _ = self.tx.send(event);
    }
}

fn take_seq(seq: &AtomicU64) -> f64 {
    (seq.fetch_add(1, Ordering::Relaxed) + 1) as f64
}

/// Resolves the signed-in login, then runs the realtime client and the kinds fetch on their
/// own thread, feeding [`InboundRx`].
pub fn spawn(config: Config) -> Result<(InboundRx, Outbound), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let me_url = format!("{}/api/me", config.web_url);
    let login = runtime
        .block_on(fetch_login(&me_url, &config.session_cookie))
        .map_err(|err| format!("GET {me_url}: {err}"))?;
    // The server only admits ids of the form `<login>#<suffix>`; a fixed suffix keeps it apart from
    // the browser's, so an office that allows several sessions per login keeps both joined.
    let self_id = format!("{login}#native");
    let (tx, rx) = mpsc::channel();
    let (out_tx, out_rx) = tokio_mpsc::unbounded_channel();
    let seq = Arc::new(AtomicU64::new(0));
    let (user_id, run_seq) = (self_id.clone(), seq.clone());
    std::thread::spawn(move || runtime.block_on(run(config, user_id, run_seq, tx, out_rx)));
    Ok((
        InboundRx {
            rx: Mutex::new(rx),
            self_id: self_id.clone(),
        },
        Outbound {
            tx: out_tx,
            seq,
            self_id,
        },
    ))
}

async fn fetch_login(url: &str, cookie: &str) -> Result<String, reqwest::Error> {
    #[derive(Deserialize)]
    struct Me {
        login: String,
    }
    let me: Me = get_json(url, cookie).await?;
    Ok(me.login)
}

async fn get_json<T: serde::de::DeserializeOwned>(
    url: &str,
    cookie: &str,
) -> Result<T, reqwest::Error> {
    reqwest::Client::new()
        .get(url)
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

async fn run(
    config: Config,
    user_id: String,
    seq: Arc<AtomicU64>,
    tx: mpsc::Sender<Inbound>,
    mut outbound: tokio_mpsc::UnboundedReceiver<NetEvent>,
) {
    let kinds_tx = tx.clone();
    let kinds_url = format!("{}/api/world-object-kinds", config.web_url);
    let cookie = config.session_cookie.clone();
    tokio::spawn(async move {
        match fetch_kinds(&kinds_url, &cookie).await {
            Ok(kinds) => {
                let _ = kinds_tx.send(Inbound::Kinds(kinds));
            }
            Err(err) => error!("GET {kinds_url}: {err}"),
        }
    });
    let layouts_tx = tx.clone();
    let (web_url, office_id, cookie) = (
        config.web_url.clone(),
        config.office_id.clone(),
        config.session_cookie.clone(),
    );
    tokio::spawn(async move {
        match fetch_layouts(&web_url, &office_id, &cookie).await {
            Ok((layouts, spawn_points)) => {
                let _ = layouts_tx.send(Inbound::Map(layouts, spawn_points));
            }
            Err(err) => error!("baked layouts: {err}"),
        }
    });

    let mut client = WsClient::connect(officexr_protocol::Config {
        ws_url: config.realtime_url,
        token_url: format!("{}/api/realtime-token", config.web_url),
        session_cookie: config.session_cookie,
        user_id: user_id.clone(),
        office_id: Some(config.office_id),
        login: None,
    });
    // The client reconnects by itself; an error or end here is final.
    loop {
        let received = tokio::select! {
            received = client.recv() => received,
            Some(event) = outbound.recv() => {
                client.broadcast(event);
                continue;
            }
        };
        let incoming = match received {
            Some(Ok(incoming)) => incoming,
            Some(Err(err)) => {
                let _ = tx.send(Inbound::Closed(err.to_string()));
                return;
            }
            None => {
                let _ = tx.send(Inbound::Closed("connection closed".into()));
                return;
            }
        };
        // The server's snapshot has no `world:objects`; a peer's snapshot offer does.
        if matches!(incoming, Incoming::Joined { .. }) {
            client.broadcast(snapshot_request(&user_id, take_seq(&seq)));
        }
        if tx.send(Inbound::Net(incoming)).is_err() {
            return;
        }
    }
}

async fn fetch_kinds(url: &str, cookie: &str) -> Result<Vec<Value>, reqwest::Error> {
    #[derive(Deserialize)]
    struct Catalog {
        kinds: Vec<Value>,
    }
    let catalog: Catalog = get_json(url, cookie).await?;
    Ok(catalog.kinds)
}

/// The office map's baked layout placements, served decoded for Bevy's glTF loader.
/// `{web_url}/api/{segments...}`, with each segment percent-encoded.
fn api_url(web_url: &str, segments: &[&str]) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(web_url).map_err(|err| format!("{web_url}: {err}"))?;
    url.path_segments_mut()
        .map_err(|()| format!("{web_url}: not a base URL"))?
        .pop_if_empty()
        .push("api")
        .extend(segments);
    Ok(url)
}

async fn fetch_layouts(
    web_url: &str,
    office_id: &str,
    cookie: &str,
) -> Result<(Vec<Layout>, Vec<SimVec3>), String> {
    #[derive(Deserialize)]
    struct Office {
        map_name: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Map {
        rooms: Vec<RoomInstance>,
        #[serde(default)]
        spawn_points: Vec<SpawnPoint>,
    }
    #[derive(Deserialize)]
    struct SpawnPoint {
        position: [f64; 3],
    }
    #[derive(Deserialize)]
    struct Room {
        #[serde(rename = "layoutName")]
        layout_name: Option<String>,
    }
    let office_url = api_url(web_url, &["offices", office_id])?;
    let office: Office = get_json(office_url.as_str(), cookie)
        .await
        .map_err(|err| err.to_string())?;
    // As the server's `DEFAULT_MAP`.
    let map_name = office.map_name.unwrap_or_else(|| "default".into());
    let map_url = api_url(web_url, &["maps", &map_name])?;
    let map: Map = get_json(map_url.as_str(), cookie)
        .await
        .map_err(|err| err.to_string())?;
    let mut layout_of = HashMap::new();
    for instance in &map.rooms {
        if layout_of.contains_key(&instance.room_name) {
            continue;
        }
        // As `mapLayoutPlacements`: a room that fails to load just has no layout.
        let layout = match api_url(web_url, &["rooms", &instance.room_name]) {
            Ok(url) => match get_json::<Room>(url.as_str(), cookie).await {
                Ok(room) => room.layout_name,
                Err(err) => {
                    warn!("GET {url}: {err}");
                    None
                }
            },
            Err(err) => {
                warn!("{err}");
                None
            }
        };
        layout_of.insert(instance.room_name.clone(), layout);
    }
    let spawn_points = map
        .spawn_points
        .iter()
        .map(|s| SimVec3 {
            x: s.position[0],
            y: s.position[1],
            z: s.position[2],
        })
        .collect();
    Ok((placements(&map.rooms, &layout_of), spawn_points))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoomInstance {
    id: String,
    room_name: String,
    position: [f32; 3],
    #[serde(default)]
    rotation_y: i32,
}

/// `mapLayoutPlacements` for a v2 map, which is what `/api/maps` serves.
fn placements(rooms: &[RoomInstance], layout_of: &HashMap<String, Option<String>>) -> Vec<Layout> {
    rooms
        .iter()
        .filter_map(|instance| {
            let layout = layout_of.get(&instance.room_name)?.as_ref()?;
            Some(Layout {
                room_instance_id: instance.id.clone(),
                placement: BakedLayoutPlacement {
                    gltf_path: format!("/api/baked-layouts/{layout}.decoded.glb"),
                    position: instance.position,
                    rotation_y: instance.rotation_y,
                },
            })
        })
        .collect()
}

fn snapshot_request(actor_id: &str, seq: f64) -> NetEvent {
    serde_json::from_value(json!({
        "kind": "snapshot:request", "v": 1, "actorId": actor_id, "seq": seq, "t": now_ms(),
    }))
    .expect("snapshot:request matches the schema")
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as f64
}

pub fn receive_inbound(
    inbound: Res<InboundRx>,
    mut world: MessageWriter<WorldUpdate>,
    mut peers: MessageWriter<PeerUpdate>,
    mut joined: MessageWriter<Joined>,
    mut exit: MessageWriter<AppExit>,
) {
    let rx = inbound.rx.lock().unwrap();
    for message in rx.try_iter() {
        match message {
            Inbound::Kinds(kinds) => {
                world.write(WorldUpdate::Kinds(kinds));
            }
            Inbound::Map(layouts, spawn_points) => {
                world.write(WorldUpdate::Map {
                    layouts,
                    spawn_points,
                });
            }
            Inbound::Net(Incoming::Joined { present }) => {
                peers.write(PeerUpdate::Present(present));
                joined.write(Joined);
            }
            Inbound::Net(Incoming::Presence { left, .. }) => {
                peers.write(PeerUpdate::Left(left));
            }
            Inbound::Net(Incoming::Event(event)) => {
                if let Some(routed) = route(*event, &inbound.self_id) {
                    match routed {
                        Routed::World(update) => {
                            world.write(update);
                        }
                        Routed::Peer(update) => {
                            peers.write(update);
                        }
                    }
                }
            }
            Inbound::Closed(reason) => {
                error!("realtime: {reason}");
                exit.write(AppExit::error());
            }
        }
    }
}

// The render crate has its own serde types for these payloads, so they cross as JSON.
fn value(payload: &impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(payload).expect("protocol payloads serialize")
}

enum Routed {
    World(WorldUpdate),
    Peer(PeerUpdate),
}

/// The `kind` patterns pin each generated variant to its wire kind, so a schema reorder that
/// renumbers the variants fails to compile here instead of misrouting.
fn route(event: NetEvent, self_id: &str) -> Option<Routed> {
    Some(match event {
        NetEvent::Variant0 {
            kind: NetEventVariant0Kind::PresencePosition,
            actor_id,
            pos,
            vel,
            yaw,
            is_airborne,
            ..
        } => Routed::Peer(PeerUpdate::Position(
            actor_id,
            Position {
                pos: SimVec3 {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                },
                vel: SimVec3 {
                    x: vel.x,
                    y: vel.y,
                    z: vel.z,
                },
                yaw,
                is_airborne,
            },
        )),
        NetEvent::Variant3 {
            kind: NetEventVariant3Kind::AvatarUpdate,
            actor_id,
            avatar,
            name,
            ..
        } => Routed::Peer(PeerUpdate::Avatar {
            id: actor_id,
            model: Some(avatar.model),
            name: name.to_string(),
        }),
        NetEvent::Variant10 {
            kind: NetEventVariant10Kind::SnapshotOffer,
            target,
            state,
            ..
        } if target == self_id => Routed::World(WorldUpdate::Objects(value(&state.world_objects))),
        NetEvent::Variant11 {
            kind: NetEventVariant11Kind::WorldConstants,
            constants,
            ..
        } => Routed::World(WorldUpdate::Constants(value(&constants))),
        NetEvent::Variant14 {
            kind: NetEventVariant14Kind::WorldObjects,
            objects,
            ..
        } => Routed::World(WorldUpdate::Objects(value(&objects))),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> NetEvent {
        let path = format!(
            "{}/../officexr-protocol/tests/fixtures/event-{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn routes_positions_and_avatars_to_peers() {
        let Some(Routed::Peer(PeerUpdate::Position(id, position))) =
            route(fixture("presence_position"), "me")
        else {
            panic!("expected a position");
        };
        assert_eq!(id, "alice");
        assert_eq!((position.pos.z, position.yaw), (3.0, 0.5));
        assert!(matches!(
            route(fixture("avatar_update"), "me"),
            Some(Routed::Peer(PeerUpdate::Avatar { model: Some(_), .. }))
        ));
    }

    #[test]
    fn routes_world_events_and_only_offers_targeted_at_self() {
        for name in ["world_constants", "world_objects"] {
            assert!(matches!(route(fixture(name), "me"), Some(Routed::World(_))));
        }
        assert!(matches!(
            route(fixture("snapshot_offer"), "bob"),
            Some(Routed::World(WorldUpdate::Objects(_)))
        ));
        assert!(route(fixture("snapshot_offer"), "me").is_none());
        assert!(route(fixture("chat_message"), "me").is_none());
    }

    #[test]
    fn places_only_rooms_that_link_a_layout() {
        let rooms: Vec<RoomInstance> = serde_json::from_value(json!([
            { "id": "r1", "roomName": "hall", "position": [1, 0, 2], "rotationY": 1 },
            { "id": "r2", "roomName": "plain", "position": [0, 0, 0] },
            { "id": "r3", "roomName": "missing", "position": [0, 0, 0] },
        ]))
        .unwrap();
        let layout_of = HashMap::from([
            ("hall".to_string(), Some("lobby".to_string())),
            ("plain".to_string(), None),
        ]);
        let placed = placements(&rooms, &layout_of);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].room_instance_id, "r1");
        let placement = &placed[0].placement;
        assert_eq!(placement.gltf_path, "/api/baked-layouts/lobby.decoded.glb");
        assert_eq!(
            (placement.position, placement.rotation_y),
            ([1.0, 0.0, 2.0], 1)
        );
    }
}
