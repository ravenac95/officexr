use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{Message, protocol::CloseFrame};

use crate::{Frame, FrameVariant0T, FrameVariant3T, FrameVariant4T, NetEvent, PresenceData};

const REPLACED_CLOSE_CODE: u16 = 4001;
const REJECTED_CLOSE_CODE: u16 = 4003;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Config {
    pub ws_url: String,
    pub token_url: String,
    /// Sent verbatim as the `Cookie` header when fetching a join token.
    pub session_cookie: String,
    pub user_id: String,
    pub office_id: Option<String>,
    pub login: Option<String>,
}

#[derive(Debug)]
pub enum Error {
    /// Close code 4001: a newer session of the same login took over. The client does not reconnect.
    Replaced,
    /// Close code 4003: the server refused the `hello`. The client does not reconnect.
    Rejected(String),
    Token(reqwest::Error),
    Ws(Box<tokio_tungstenite::tungstenite::Error>),
    Closed,
    HandshakeTimeout,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Replaced => write!(f, "replaced by a newer session"),
            Self::Rejected(reason) => write!(f, "rejected by the server: {reason}"),
            Self::Token(err) => write!(f, "token fetch failed: {err}"),
            Self::Ws(err) => write!(f, "websocket error: {err}"),
            Self::Closed => write!(f, "connection closed"),
            Self::HandshakeTimeout => write!(f, "handshake timed out"),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Self::Token(err)
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Ws(Box::new(err))
    }
}

#[derive(Debug)]
pub enum Incoming {
    /// A `hello-ack` was received; sent again after every reconnect.
    Joined {
        present: Vec<String>,
    },
    Event(Box<NetEvent>),
    Presence {
        joined: Vec<String>,
        left: Vec<String>,
    },
}

/// Last presence set by the caller; survives disconnects so it can be re-sent after the next hello-ack.
type Presence = Arc<Mutex<Option<PresenceData>>>;

enum Outgoing {
    Broadcast(Box<NetEvent>),
    TrackPresence,
}

/// Joins a room and keeps the connection alive, fetching a fresh token before every `hello`.
/// Only the fatal errors [`Error::Replaced`] and [`Error::Rejected`] are surfaced; everything
/// else is retried with backoff.
pub struct WsClient {
    outgoing: mpsc::UnboundedSender<Outgoing>,
    presence: Presence,
    incoming: mpsc::UnboundedReceiver<Result<Incoming, Error>>,
}

impl WsClient {
    pub fn connect(config: Config) -> Self {
        let (outgoing, out_rx) = mpsc::unbounded_channel();
        let (in_tx, incoming) = mpsc::unbounded_channel();
        let presence = Presence::default();
        tokio::spawn(run(config, presence.clone(), out_rx, in_tx));
        Self {
            outgoing,
            presence,
            incoming,
        }
    }

    /// Dropped when not connected, like the browser channel.
    pub fn broadcast(&self, event: NetEvent) {
        let _ = self.outgoing.send(Outgoing::Broadcast(Box::new(event)));
    }

    /// Remembered, even while disconnected, and sent after every hello-ack.
    pub fn track_presence(&self, data: PresenceData) {
        *self.presence.lock().unwrap() = Some(data);
        let _ = self.outgoing.send(Outgoing::TrackPresence);
    }

    /// `None` once the client has stopped after a fatal error.
    pub async fn recv(&mut self) -> Option<Result<Incoming, Error>> {
        self.incoming.recv().await
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn run(
    config: Config,
    presence: Presence,
    mut out_rx: mpsc::UnboundedReceiver<Outgoing>,
    in_tx: mpsc::UnboundedSender<Result<Incoming, Error>>,
) {
    let http = reqwest::Client::new();
    let mut attempt = 0u32;
    loop {
        let err = session(&config, &http, &mut out_rx, &in_tx, &presence, &mut attempt).await;
        if matches!(err, Error::Replaced | Error::Rejected(_)) {
            let _ = in_tx.send(Err(err));
            return;
        }
        if in_tx.is_closed() {
            return;
        }
        // ponytail: no jitter, so clients restarted together reconnect in lockstep.
        let delay = Duration::from_secs(1 << attempt.min(5)).min(Duration::from_secs(30));
        attempt += 1;
        log::warn!("realtime session ended ({err}); reconnecting in {delay:?}");
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = in_tx.closed() => return,
        }
    }
}

/// Runs one connection until it ends; the returned error says why.
async fn session(
    config: &Config,
    http: &reqwest::Client,
    out_rx: &mut mpsc::UnboundedReceiver<Outgoing>,
    in_tx: &mpsc::UnboundedSender<Result<Incoming, Error>>,
    presence: &Presence,
    attempt: &mut u32,
) -> Error {
    let mut ws = match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake(config, http, in_tx)).await
    {
        Ok(Ok(ws)) => ws,
        Ok(Err(err)) => return err,
        Err(_) => return Error::HandshakeTimeout,
    };
    *attempt = 0;
    while out_rx.try_recv().is_ok() {}
    if let Err(err) = send_presence(&mut ws, presence).await {
        return err;
    }
    loop {
        tokio::select! {
            message = ws.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    for incoming in decode(&text) {
                        if in_tx.send(Ok(incoming)).is_err() {
                            return Error::Closed;
                        }
                    }
                }
                Some(Ok(Message::Close(frame))) => return close_error(frame),
                Some(Ok(_)) => {}
                Some(Err(err)) => return err.into(),
                None => return Error::Closed,
            },
            outgoing = out_rx.recv() => {
                let Some(outgoing) = outgoing else { return Error::Closed };
                let frame = match outgoing {
                    Outgoing::Broadcast(event) => Frame::Variant3 {
                        event: *event,
                        from: config.user_id.clone(),
                        t: FrameVariant3T::Broadcast,
                    },
                    Outgoing::TrackPresence => {
                        if let Err(err) = send_presence(&mut ws, presence).await {
                            return err;
                        }
                        continue;
                    }
                };
                if let Err(err) = send(&mut ws, frame).await {
                    return err;
                }
            }
        }
    }
}

/// Fetches a token, sends `hello` and returns the socket once `hello-ack` has arrived.
async fn handshake(
    config: &Config,
    http: &reqwest::Client,
    in_tx: &mpsc::UnboundedSender<Result<Incoming, Error>>,
) -> Result<Socket, Error> {
    #[derive(Deserialize)]
    struct TokenResponse {
        token: String,
    }
    let TokenResponse { token } = http
        .get(&config.token_url)
        .header(reqwest::header::COOKIE, &config.session_cookie)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let (mut ws, _) = tokio_tungstenite::connect_async(&config.ws_url).await?;
    let hello = Frame::Variant0 {
        login: config.login.clone(),
        office_id: config.office_id.clone(),
        t: FrameVariant0T::Hello,
        token: Some(token),
        user_id: config.user_id.clone(),
    };
    send(&mut ws, hello).await?;
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(text))) => {
                let incomings = decode(&text);
                let acked = incomings
                    .iter()
                    .any(|i| matches!(i, Incoming::Joined { .. }));
                for incoming in incomings {
                    let _ = in_tx.send(Ok(incoming));
                }
                if acked {
                    return Ok(ws);
                }
            }
            Some(Ok(Message::Close(frame))) => return Err(close_error(frame)),
            Some(Ok(_)) => {}
            Some(Err(err)) => return Err(err.into()),
            None => return Err(Error::Closed),
        }
    }
}

async fn send_presence(ws: &mut Socket, presence: &Presence) -> Result<(), Error> {
    let data = presence.lock().unwrap().clone();
    match data {
        Some(data) => {
            send(
                ws,
                Frame::Variant4 {
                    data,
                    t: FrameVariant4T::TrackPresence,
                },
            )
            .await
        }
        None => Ok(()),
    }
}

async fn send(ws: &mut Socket, frame: Frame) -> Result<(), Error> {
    let text = serde_json::to_string(&frame).expect("frames serialize");
    Ok(ws.send(Message::text(text)).await?)
}

pub(crate) fn close_error(frame: Option<CloseFrame>) -> Error {
    match frame {
        Some(frame) if u16::from(frame.code) == REPLACED_CLOSE_CODE => Error::Replaced,
        Some(frame) if u16::from(frame.code) == REJECTED_CLOSE_CODE => {
            Error::Rejected(frame.reason.to_string())
        }
        _ => Error::Closed,
    }
}

/// Turns one inbound text frame into what the caller should see. Events with an unknown `kind`
/// or a `v` other than the generated schema's fail to deserialize and are logged and dropped.
pub(crate) fn decode(text: &str) -> Vec<Incoming> {
    let mut value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(err) => {
            log::warn!("dropping malformed frame: {err}");
            return vec![];
        }
    };
    match value["t"].as_str() {
        Some("snapshot") => match value["events"].take() {
            Value::Array(events) => events.iter().filter_map(decode_event).collect(),
            _ => vec![],
        },
        Some("broadcast") => decode_event(&value["event"]).into_iter().collect(),
        _ => match Frame::deserialize(&value) {
            Ok(Frame::Variant1 { present, .. }) => vec![Incoming::Joined { present }],
            Ok(Frame::Variant5 { joined, left, .. }) => vec![Incoming::Presence { joined, left }],
            Ok(_) => vec![],
            Err(err) => {
                log::warn!("dropping unrecognised frame: {err}");
                vec![]
            }
        },
    }
}

fn decode_event(value: &Value) -> Option<Incoming> {
    NetEvent::deserialize(value)
        .inspect_err(|err| {
            log::warn!(
                "dropping event kind={} v={}: {err}",
                value["kind"],
                value["v"]
            )
        })
        .ok()
        .map(|event| Incoming::Event(Box::new(event)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, v: u32) -> String {
        format!(r#"{{"kind":"{kind}","v":{v},"actorId":"a","seq":1,"t":1,"text":"hi"}}"#)
    }

    #[test]
    fn broadcast_with_known_kind_is_delivered() {
        let frame = format!(
            r#"{{"t":"broadcast","from":"a","event":{}}}"#,
            event("chat:message", 1)
        );
        assert!(matches!(decode(&frame)[..], [Incoming::Event(_)]));
    }

    #[test]
    fn unknown_kind_and_mismatched_version_are_dropped() {
        for bad in [event("chat:shout", 1), event("chat:message", 2)] {
            let frame = format!(r#"{{"t":"broadcast","from":"a","event":{bad}}}"#);
            assert!(decode(&frame).is_empty(), "{bad}");
        }
    }

    #[test]
    fn snapshot_keeps_good_events_around_bad_ones() {
        let events = [
            event("chat:shout", 1),
            event("chat:message", 1),
            event("chat:message", 9),
        ]
        .join(",");
        let frame = format!(r#"{{"t":"snapshot","events":[{events}]}}"#);
        assert!(matches!(decode(&frame)[..], [Incoming::Event(_)]));
    }

    fn close(code: u16, reason: &str) -> Option<CloseFrame> {
        Some(CloseFrame {
            code: code.into(),
            reason: reason.into(),
        })
    }

    #[test]
    fn close_codes_map_to_distinct_errors() {
        assert!(matches!(close_error(close(4001, "")), Error::Replaced));
        assert!(
            matches!(close_error(close(4003, "bad token")), Error::Rejected(r) if r == "bad token")
        );
        assert!(matches!(close_error(close(1000, "")), Error::Closed));
        assert!(matches!(close_error(None), Error::Closed));
    }

    #[test]
    fn garbage_is_dropped() {
        assert!(decode("not json").is_empty());
        assert!(decode(r#"{"t":"nope"}"#).is_empty());
    }
}
