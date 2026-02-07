use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMeta {
    pub device_id: Option<String>,
    pub hostname: Option<String>,
    pub user_id: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub peer_id: String,
    pub addrs: Vec<String>,
    #[serde(default)]
    pub meta: Option<PeerMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub ok: bool,
    pub ttl_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub addrs: Vec<String>,
    pub meta: Option<PeerMeta>,
    pub last_seen_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResponse {
    pub peers: Vec<PeerInfo>,
}

#[derive(Debug, Clone)]
struct PeerRecord {
    addrs: Vec<String>,
    meta: Option<PeerMeta>,
    last_seen_unix: i64,
}

#[derive(Debug, Default)]
struct TrackerState {
    peers: HashMap<String, PeerRecord>,
}

pub fn serve(bind: &str, ttl_sec: u64, token: Option<String>) -> Result<()> {
    let state = Arc::new(Mutex::new(TrackerState::default()));
    serve_http(bind, ttl_sec, token, state)
}

fn serve_http(
    bind: &str,
    ttl_sec: u64,
    token: Option<String>,
    state: Arc<Mutex<TrackerState>>,
) -> Result<()> {
    let server =
        tiny_http::Server::http(bind).map_err(|e| anyhow::anyhow!("listen {bind}: {e}"))?;

    for mut req in server.incoming_requests() {
        let res = route_http_request(&state, ttl_sec, token.as_deref(), &mut req)
            .unwrap_or_else(|err| respond_text(500, &format!("error: {err:#}\n")));
        let _ = req.respond(res);
    }

    Ok(())
}

fn route_http_request(
    state: &Arc<Mutex<TrackerState>>,
    ttl_sec: u64,
    token: Option<&str>,
    req: &mut tiny_http::Request,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    if token.is_some() && !is_authorized(req, token.unwrap()) {
        return Ok(respond_text(401, "unauthorized\n"));
    }

    let url = req.url().to_string();
    let method = req.method().as_str();

    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url.as_str(), None),
    };

    match (method, path) {
        ("GET", "/api/v1/ping") => Ok(respond_text(200, "ok\n")),
        ("POST", "/api/v1/peers/register") => {
            let mut buf = Vec::new();
            req.as_reader()
                .read_to_end(&mut buf)
                .context("read request body")?;

            let reg: RegisterRequest =
                serde_json::from_slice(&buf).context("parse register request json")?;
            let peer_id = reg.peer_id.trim().to_string();
            if peer_id.is_empty() {
                return Ok(respond_text(400, "peer_id required\n"));
            }

            let now = OffsetDateTime::now_utc();
            {
                let mut locked = state.lock().unwrap();
                prune_expired(&mut locked, now, ttl_sec);
                locked.peers.insert(
                    peer_id,
                    PeerRecord {
                        addrs: reg
                            .addrs
                            .into_iter()
                            .map(|a| a.trim().to_string())
                            .filter(|a| !a.is_empty())
                            .collect(),
                        meta: reg.meta,
                        last_seen_unix: now.unix_timestamp(),
                    },
                );
            }

            respond_json(200, &RegisterResponse { ok: true, ttl_sec })
        }
        ("GET", "/api/v1/peers") => {
            let user_id = query.and_then(|q| query_get(q, "user_id"));
            let now = OffsetDateTime::now_utc();

            let peers = {
                let mut locked = state.lock().unwrap();
                prune_expired(&mut locked, now, ttl_sec);
                locked
                    .peers
                    .iter()
                    .filter(|(_, rec)| match (user_id, &rec.meta) {
                        (None, _) => true,
                        (Some(want), Some(meta)) => meta.user_id.as_deref() == Some(want),
                        (Some(_), None) => false,
                    })
                    .map(|(peer_id, rec)| PeerInfo {
                        peer_id: peer_id.clone(),
                        addrs: rec.addrs.clone(),
                        meta: rec.meta.clone(),
                        last_seen_unix: rec.last_seen_unix,
                    })
                    .collect::<Vec<_>>()
            };

            respond_json(200, &ListResponse { peers })
        }
        _ => Ok(respond_text(404, "not found\n")),
    }
}

fn prune_expired(state: &mut TrackerState, now: OffsetDateTime, ttl_sec: u64) {
    if ttl_sec == 0 {
        state.peers.clear();
        return;
    }

    let now_ts = now.unix_timestamp();
    let ttl = ttl_sec as i64;
    state
        .peers
        .retain(|_, rec| now_ts - rec.last_seen_unix <= ttl);
}

fn query_get<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for part in query.split('&') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if k == key {
            return Some(v);
        }
    }
    None
}

fn is_authorized(req: &tiny_http::Request, token: &str) -> bool {
    let token = token.trim();
    if token.is_empty() {
        return true;
    }

    // 1) Authorization: Bearer <token>
    if let Some(value) = header_value(req, "Authorization")
        && let Some(rest) = value.strip_prefix("Bearer ")
    {
        return rest.trim() == token;
    }

    // 2) X-Rustory-Token: <token>
    if let Some(value) = header_value(req, "X-Rustory-Token") {
        return value.trim() == token;
    }

    false
}

fn header_value(req: &tiny_http::Request, name: &'static str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

#[derive(Clone)]
pub struct TrackerClient {
    base_url: String,
    token: Option<String>,
}

impl TrackerClient {
    pub fn new(base_url: String, token: Option<String>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
        }
    }

    pub fn register(&self, req: &RegisterRequest) -> Result<RegisterResponse> {
        let url = format!("{}/api/v1/peers/register", self.base_url);
        let body = serde_json::to_vec(req).context("serialize register request")?;

        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(10))
            .build();

        let mut r = agent.post(&url).set("Content-Type", "application/json");
        if let Some(token) = &self.token {
            r = r.set("Authorization", &format!("Bearer {}", token.trim()));
        }

        let resp = r.send_bytes(&body).with_context(|| format!("POST {url}"))?;
        let text = resp.into_string().context("read response body")?;
        serde_json::from_str(&text).context("parse register response json")
    }

    pub fn list(&self, user_id: Option<&str>) -> Result<ListResponse> {
        let mut url = format!("{}/api/v1/peers", self.base_url);
        if let Some(user_id) = user_id {
            url = format!("{url}?user_id={user_id}");
        }

        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(10))
            .build();

        let mut r = agent.get(&url);
        if let Some(token) = &self.token {
            r = r.set("Authorization", &format!("Bearer {}", token.trim()));
        }

        let resp = r.call().with_context(|| format!("GET {url}"))?;
        let text = resp.into_string().context("read response body")?;
        serde_json::from_str(&text).context("parse list response json")
    }
}

fn respond_text(code: u16, body: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let mut res = tiny_http::Response::from_data(body.as_bytes().to_vec());
    res = res.with_status_code(code);
    res = res.with_header(
        tiny_http::Header::from_bytes("Content-Type", "text/plain; charset=utf-8").unwrap(),
    );
    res
}

fn respond_json<T: Serialize>(
    code: u16,
    value: &T,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let body = serde_json::to_vec(value).context("serialize json")?;
    let mut res = tiny_http::Response::from_data(body);
    res = res.with_status_code(code);
    res =
        res.with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap());
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    struct TestServer {
        base_url: String,
        shutdown: Arc<AtomicBool>,
        join: Option<thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn shutdown(mut self) {
            self.shutdown.store(true, Ordering::SeqCst);
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    fn start_test_server(ttl_sec: u64, token: Option<String>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let bind = format!("127.0.0.1:{}", addr.port());
        let base_url = format!("http://{}", bind);

        let state = Arc::new(Mutex::new(TrackerState::default()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown2 = shutdown.clone();
        let token2 = token.clone();
        let state2 = state.clone();

        let join = thread::spawn(move || {
            let server = tiny_http::Server::http(&bind).unwrap();
            while !shutdown2.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(mut req)) => {
                        let res = route_http_request(&state2, ttl_sec, token2.as_deref(), &mut req)
                            .unwrap_or_else(|e| respond_text(500, &format!("error: {e:#}\n")));
                        let _ = req.respond(res);
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });

        // 서버가 뜰 때까지 짧게 대기(ping).
        for _ in 0..50 {
            let url = format!("{}/api/v1/ping", base_url);
            if ureq::get(&url).call().is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        TestServer {
            base_url,
            shutdown,
            join: Some(join),
        }
    }

    #[test]
    fn tracker_register_and_list_end_to_end() {
        let server = start_test_server(60, None);
        let client = TrackerClient::new(server.base_url.clone(), None);

        let req = RegisterRequest {
            peer_id: "peer-a".to_string(),
            addrs: vec!["/ip4/127.0.0.1/tcp/1234".to_string()],
            meta: Some(PeerMeta {
                user_id: Some("u1".to_string()),
                device_id: Some("d1".to_string()),
                hostname: None,
                version: Some("0.1.0".to_string()),
            }),
        };
        let resp = client.register(&req).unwrap();
        assert!(resp.ok);

        let list = client.list(Some("u1")).unwrap();
        assert_eq!(list.peers.len(), 1);
        assert_eq!(list.peers[0].peer_id, "peer-a");

        server.shutdown();
    }

    #[test]
    fn tracker_rejects_without_token() {
        let server = start_test_server(60, Some("secret".to_string()));
        let client = TrackerClient::new(server.base_url.clone(), None);

        let req = RegisterRequest {
            peer_id: "peer-a".to_string(),
            addrs: vec![],
            meta: None,
        };

        let err = client.register(&req).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("status code 401"));

        server.shutdown();
    }

    #[test]
    fn tracker_accepts_with_token() {
        let server = start_test_server(60, Some("secret".to_string()));
        let client = TrackerClient::new(server.base_url.clone(), Some("secret".to_string()));

        let req = RegisterRequest {
            peer_id: "peer-a".to_string(),
            addrs: vec![],
            meta: None,
        };
        let resp = client.register(&req).unwrap();
        assert!(resp.ok);

        server.shutdown();
    }
}
