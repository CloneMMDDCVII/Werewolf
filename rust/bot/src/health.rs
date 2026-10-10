//! `GET /healthz` for monitors and load balancers, served when
//! `health_addr` is set in `werewolf.toml`. 200 when Telegram polling has
//! succeeded recently and the database answers, 503 otherwise; the body
//! is a small JSON object saying which, plus the number of running games.
//!
//! One endpoint, so the HTTP is written by hand over a `TcpListener`
//! rather than pulling in a server framework: read one request head,
//! answer it, close the connection.

use db::Store;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;

/// Polling counts as alive if `getUpdates` succeeded this recently. A
/// long poll lasts up to 30 s, so this allows a few slow or failed ones.
pub const POLL_STALE_AFTER: Duration = Duration::from_secs(120);
/// How long the database gets to answer `SELECT 1`.
pub const DATABASE_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a client gets to send its request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST_BYTES: usize = 8 * 1024;

/// When polling last succeeded. Starts at creation, so a bot that has
/// just started is given [`POLL_STALE_AFTER`] to reach Telegram.
pub struct Heartbeat(Mutex<Instant>);

impl Heartbeat {
    pub fn new() -> Self {
        Heartbeat(Mutex::new(Instant::now()))
    }

    pub fn beat(&self) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    pub fn since(&self) -> Duration {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).elapsed()
    }
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Health {
    pub healthy: bool,
    pub polling: bool,
    pub database: bool,
    pub running_games: usize,
    /// Seconds since `getUpdates` last succeeded.
    pub last_poll_secs: u64,
}

pub fn assess(since_last_poll: Duration, database_ok: bool, running_games: usize) -> Health {
    let polling = since_last_poll <= POLL_STALE_AFTER;
    Health {
        healthy: polling && database_ok,
        polling,
        database: database_ok,
        running_games,
        last_poll_secs: since_last_poll.as_secs(),
    }
}

/// What a health check looks at.
pub struct Probe {
    heartbeat: Arc<Heartbeat>,
    store: Store,
    running_games: Box<dyn Fn() -> usize + Send + Sync>,
}

impl Probe {
    pub fn new(
        heartbeat: Arc<Heartbeat>,
        store: Store,
        running_games: impl Fn() -> usize + Send + Sync + 'static,
    ) -> Self {
        Probe {
            heartbeat,
            store,
            running_games: Box::new(running_games),
        }
    }

    pub async fn check(&self) -> Health {
        let database_ok = matches!(
            tokio::time::timeout(DATABASE_TIMEOUT, self.store.ping()).await,
            Ok(Ok(()))
        );
        assess(self.heartbeat.since(), database_ok, (self.running_games)())
    }
}

/// Answers health checks on `listener` until the task is dropped.
pub async fn serve(listener: TcpListener, probe: Arc<Probe>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let probe = probe.clone();
                tokio::spawn(async move {
                    if let Err(e) = answer(stream, &probe).await {
                        log::debug!("health check connection: {e}");
                    }
                });
            }
            Err(e) => {
                log::warn!("health endpoint accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn answer(mut stream: TcpStream, probe: &Probe) -> std::io::Result<()> {
    let head = match tokio::time::timeout(REQUEST_TIMEOUT, read_head(&mut stream)).await {
        Ok(head) => head?,
        Err(_) => return Ok(()),
    };
    let mut parts = head.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let path = path.split('?').next().unwrap_or("");
    let (status, body) = match (method, path) {
        ("GET" | "HEAD", "/healthz") => {
            let health = probe.check().await;
            let status = if health.healthy {
                "200 OK"
            } else {
                "503 Service Unavailable"
            };
            (status, serde_json::to_string(&health).unwrap_or_default())
        }
        (_, "/healthz") => ("405 Method Not Allowed", String::new()),
        _ => ("404 Not Found", String::new()),
    };
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if method != "HEAD" {
        response.push_str(&body);
    }
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

/// Reads up to the blank line ending the request head and returns its
/// first line. Any body is ignored.
async fn read_head(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && !buf.windows(2).any(|w| w == b"\n\n") {
        let n = stream.read(&mut chunk).await?;
        if n == 0 || buf.len() + n > MAX_REQUEST_BYTES {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&buf);
    Ok(text.lines().next().unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_needs_recent_polling_and_the_database() {
        let fresh = Duration::from_secs(5);
        let stale = POLL_STALE_AFTER + Duration::from_secs(1);
        assert_eq!(
            assess(fresh, true, 3),
            Health {
                healthy: true,
                polling: true,
                database: true,
                running_games: 3,
                last_poll_secs: 5,
            }
        );
        let h = assess(stale, true, 0);
        assert!(!h.healthy && !h.polling && h.database);
        let h = assess(fresh, false, 0);
        assert!(!h.healthy && h.polling && !h.database);
        assert!(
            assess(POLL_STALE_AFTER, true, 0).healthy,
            "the threshold itself still counts"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_heartbeat_ages_until_the_next_beat() {
        let hb = Heartbeat::new();
        tokio::time::advance(Duration::from_secs(30)).await;
        assert_eq!(hb.since(), Duration::from_secs(30));
        hb.beat();
        assert_eq!(hb.since(), Duration::ZERO);
    }

    async fn get(addr: std::net::SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn the_endpoint_reports_over_http() {
        let store = Store::in_memory().await.unwrap();
        let probe = Arc::new(Probe::new(Arc::new(Heartbeat::new()), store.clone(), || 2));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve(listener, probe));

        let ok = get(addr, "GET /healthz HTTP/1.1\r\nHost: x\r\n\r\n").await;
        assert!(ok.starts_with("HTTP/1.1 200 OK\r\n"), "{ok}");
        assert!(ok.ends_with(r#""healthy":true,"polling":true,"database":true,"running_games":2,"last_poll_secs":0}"#));
        assert!(get(addr, "GET / HTTP/1.1\r\n\r\n").await.starts_with("HTTP/1.1 404"));
        assert!(get(addr, "POST /healthz HTTP/1.1\r\n\r\n")
            .await
            .starts_with("HTTP/1.1 405"));

        store.close().await;
        let down = get(addr, "GET /healthz HTTP/1.0\r\n\r\n").await;
        assert!(down.starts_with("HTTP/1.1 503 "), "{down}");
        assert!(down.contains(r#""database":false"#));
        server.abort();
    }
}
