mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use common::{COOKIE, config, next, token_stub};
use futures_util::{SinkExt, StreamExt};
use officexr_protocol::{Error, Incoming, PresenceData, WsClient};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    WebSocketStream, accept_async,
    tungstenite::{
        Message,
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

type Conn = WebSocketStream<tokio::net::TcpStream>;

async fn read_json(conn: &mut Conn) -> Value {
    loop {
        if let Message::Text(text) = conn.next().await.unwrap().unwrap() {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

async fn ack(conn: &mut Conn) {
    assert_eq!(read_json(conn).await["t"], "hello");
    conn.send(Message::text(
        json!({"t": "hello-ack", "present": []}).to_string(),
    ))
    .await
    .unwrap();
}

/// Serves websocket connections one at a time with `handler(connection_index, socket)`.
async fn serve<F, Fut>(handler: F) -> u16
where
    F: Fn(usize, Conn) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut index = 0;
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            handler(index, accept_async(stream).await.unwrap()).await;
            index += 1;
        }
    });
    port
}

async fn client(ws_port: u16) -> (WsClient, Arc<Mutex<Vec<String>>>) {
    let cookies = Arc::new(Mutex::new(vec![]));
    let token_port = token_stub(cookies.clone()).await;
    (
        WsClient::connect(config("alice", ws_port, token_port)),
        cookies,
    )
}

#[tokio::test]
async fn presence_set_before_connecting_is_sent_after_hello_ack() {
    let (frames_tx, mut frames) = tokio::sync::mpsc::unbounded_channel();
    let port = serve(move |_, mut conn| {
        let frames_tx = frames_tx.clone();
        async move {
            ack(&mut conn).await;
            frames_tx.send(read_json(&mut conn).await).unwrap();
            conn.next().await;
        }
    })
    .await;
    let (alice, _) = client(port).await;
    let data: PresenceData = serde_json::from_value(json!({"cursor": [1, 2]})).unwrap();
    alice.track_presence(data);
    let frame = tokio::time::timeout(Duration::from_secs(10), frames.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        frame,
        json!({"t": "track-presence", "data": {"cursor": [1, 2]}})
    );
}

#[tokio::test]
async fn close_codes_4001_and_4003_are_distinct_fatal_errors() {
    for (code, reason) in [(4001, ""), (4003, "bad token")] {
        let port = serve(move |_, mut conn| async move {
            ack(&mut conn).await;
            let frame = CloseFrame {
                code: CloseCode::Library(code),
                reason: reason.into(),
            };
            conn.close(Some(frame)).await.unwrap();
        })
        .await;
        let (mut alice, _) = client(port).await;
        assert!(matches!(next(&mut alice).await, Incoming::Joined { .. }));
        let err = tokio::time::timeout(Duration::from_secs(10), alice.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        match code {
            4001 => assert!(matches!(err, Error::Replaced)),
            _ => assert!(matches!(err, Error::Rejected(ref r) if r == "bad token")),
        }
        assert!(
            alice.recv().await.is_none(),
            "client stops after a fatal close"
        );
    }
}

#[tokio::test]
async fn reconnect_fetches_a_fresh_token() {
    let port = serve(|index, mut conn| async move {
        ack(&mut conn).await;
        if index == 0 {
            conn.close(None).await.unwrap();
        } else {
            conn.next().await;
        }
    })
    .await;
    let (mut alice, cookies) = client(port).await;
    for _ in 0..2 {
        while !matches!(next(&mut alice).await, Incoming::Joined { .. }) {}
    }
    assert_eq!(cookies.lock().unwrap().as_slice(), [COOKIE, COOKIE]);
}
