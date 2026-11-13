use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use officexr_protocol::{Config, Incoming, WsClient};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

pub const COOKIE: &str = "session=abc";

/// Stands in for the web app's `/api/realtime-token`; records each request's `Cookie` header.
pub async fn token_stub(cookies: Arc<Mutex<Vec<String>>>) -> u16 {
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

pub async fn next(client: &mut WsClient) -> Incoming {
    timeout(Duration::from_secs(10), client.recv())
        .await
        .expect("timed out")
        .expect("client stopped")
        .expect("fatal error")
}

pub fn config(user: &str, ws_port: u16, token_port: u16) -> Config {
    Config {
        ws_url: format!("ws://127.0.0.1:{ws_port}/ws"),
        token_url: format!("http://127.0.0.1:{token_port}/api/realtime-token"),
        session_cookie: COOKIE.into(),
        user_id: user.into(),
        office_id: Some("main".into()),
        login: None,
    }
}
