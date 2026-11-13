use std::sync::{Mutex, mpsc};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use officexr_protocol::{Incoming, NetEvent, WsClient};
use officexr_render::WorldObjectKind;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::Config;
use crate::peers::PeerUpdate;
use crate::world::WorldUpdate;

pub enum Inbound {
    Kinds(Vec<WorldObjectKind>),
    Net(Incoming),
}

#[derive(Resource)]
pub struct InboundRx {
    rx: Mutex<mpsc::Receiver<Inbound>>,
    self_id: String,
}

/// Resolves the signed-in login, then runs the realtime client and the kinds fetch on their
/// own thread, feeding [`InboundRx`].
pub fn spawn(config: Config) -> Result<InboundRx, String> {
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
    let user_id = self_id.clone();
    std::thread::spawn(move || runtime.block_on(run(config, user_id, tx)));
    Ok(InboundRx {
        rx: Mutex::new(rx),
        self_id,
    })
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

async fn run(config: Config, user_id: String, tx: mpsc::Sender<Inbound>) {
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

    let mut client = WsClient::connect(officexr_protocol::Config {
        ws_url: config.realtime_url,
        token_url: format!("{}/api/realtime-token", config.web_url),
        session_cookie: config.session_cookie,
        user_id: user_id.clone(),
        office_id: Some(config.office_id),
        login: None,
    });
    let mut seq = 0.0;
    while let Some(incoming) = client.recv().await {
        let incoming = match incoming {
            Ok(incoming) => incoming,
            Err(err) => {
                error!("realtime: {err}");
                return;
            }
        };
        // The server's snapshot has no `world:objects`; a peer's snapshot offer does.
        if matches!(incoming, Incoming::Joined { .. }) {
            seq += 1.0;
            client.broadcast(snapshot_request(&user_id, seq));
        }
        if tx.send(Inbound::Net(incoming)).is_err() {
            return;
        }
    }
}

async fn fetch_kinds(url: &str, cookie: &str) -> Result<Vec<WorldObjectKind>, reqwest::Error> {
    #[derive(Deserialize)]
    struct Catalog {
        kinds: Vec<WorldObjectKind>,
    }
    let catalog: Catalog = get_json(url, cookie).await?;
    Ok(catalog.kinds)
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
) {
    let rx = inbound.rx.lock().unwrap();
    for message in rx.try_iter() {
        match message {
            Inbound::Kinds(kinds) => {
                world.write(WorldUpdate::Kinds(kinds));
            }
            Inbound::Net(Incoming::Joined { present }) => {
                peers.write(PeerUpdate::Present(present));
            }
            Inbound::Net(Incoming::Presence { left, .. }) => {
                peers.write(PeerUpdate::Left(left));
            }
            Inbound::Net(Incoming::Event(event)) => {
                let event = serde_json::to_value(*event).expect("events serialize");
                route(event, &inbound.self_id, &mut world, &mut peers);
            }
        }
    }
}

fn route(
    mut event: Value,
    self_id: &str,
    world: &mut MessageWriter<WorldUpdate>,
    peers: &mut MessageWriter<PeerUpdate>,
) {
    let actor = event["actorId"].as_str().unwrap_or_default().to_string();
    match event["kind"].as_str().unwrap_or_default() {
        "world:objects" => {
            world.write(WorldUpdate::Objects(event["objects"].take()));
        }
        "world:constants" => {
            world.write(WorldUpdate::Constants(event["constants"].take()));
        }
        "snapshot:offer" if event["target"] == self_id => {
            world.write(WorldUpdate::Objects(event["state"]["worldObjects"].take()));
        }
        "presence:position" => {
            if let Ok(position) = serde_json::from_value(event) {
                peers.write(PeerUpdate::Position(actor, position));
            }
        }
        "avatar:update" => {
            peers.write(PeerUpdate::Avatar {
                id: actor,
                model: event["avatar"]["model"].as_str().map(Into::into),
                name: event["name"].as_str().unwrap_or_default().into(),
            });
        }
        _ => {}
    }
}
