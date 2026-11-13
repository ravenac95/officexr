use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use officexr_protocol::{
    Config, Incoming, NetEvent, NetEventVariant0Kind, NetEventVariant0Pos, NetEventVariant0Vel,
    WsClient,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    time::timeout,
};

const COOKIE: &str = "session=abc";

/// Runs the real server (no token secret, so any token is accepted) and kills it on drop.
struct Server(#[allow(dead_code)] Child);

impl Server {
    async fn start(port: u16) -> Self {
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/realtime-server");
        let child = Command::new("node")
            .args(["--import", "tsx", "src/cli.ts"])
            .current_dir(package)
            .env("REALTIME_PORT", port.to_string())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn realtime-server (run `pnpm install` first)");
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                return Self(child);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("realtime-server did not start");
    }
}

/// Stands in for the web app's `/api/realtime-token`; records each request's `Cookie` header.
async fn token_stub(cookies: Arc<Mutex<Vec<String>>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let cookies = cookies.clone();
            tokio::spawn(async move {
                let mut buf = vec![0; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let cookie = request
                    .lines()
                    .find_map(|l| l.strip_prefix("cookie: "))
                    .unwrap_or_default();
                cookies.lock().unwrap().push(cookie.to_string());
                let body = r#"{"token":"stub"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    port
}

async fn next(client: &mut WsClient) -> Incoming {
    timeout(Duration::from_secs(10), client.recv())
        .await
        .expect("timed out")
        .expect("client stopped")
        .expect("fatal error")
}

fn config(user: &str, ws_port: u16, token_port: u16) -> Config {
    Config {
        ws_url: format!("ws://127.0.0.1:{ws_port}/ws"),
        token_url: format!("http://127.0.0.1:{token_port}/api/realtime-token"),
        session_cookie: COOKIE.into(),
        user_id: user.into(),
        office_id: Some("main".into()),
        login: None,
    }
}

#[tokio::test]
async fn joins_and_exchanges_position() {
    let ws_port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let _server = Server::start(ws_port).await;
    let cookies = Arc::new(Mutex::new(vec![]));
    let token_port = token_stub(cookies.clone()).await;

    let mut alice = WsClient::connect(config("alice", ws_port, token_port));
    let mut bob = WsClient::connect(config("bob", ws_port, token_port));

    // Snapshot events (e.g. world:constants) may arrive between the frames waited for here.
    while !matches!(next(&mut alice).await, Incoming::Joined { .. }) {}
    while !matches!(next(&mut bob).await, Incoming::Joined { .. }) {}
    assert_eq!(cookies.lock().unwrap().as_slice(), [COOKIE, COOKIE]);

    bob.broadcast(NetEvent::Variant0 {
        actor_id: "bob".into(),
        is_airborne: false,
        kind: NetEventVariant0Kind::PresencePosition,
        pos: NetEventVariant0Pos {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        seq: 1.0,
        t: 1.0,
        v: 1.0.try_into().unwrap(),
        vel: NetEventVariant0Vel {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        yaw: 0.0,
    });
    loop {
        if let Incoming::Event(event) = next(&mut alice).await
            && let NetEvent::Variant0 { actor_id, pos, .. } = *event
        {
            assert_eq!((actor_id.as_str(), pos.x, pos.z), ("bob", 1.0, 3.0));
            break;
        }
    }
}
