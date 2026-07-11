use crate::{
    core::{Entry, EntryDeletion},
    storage::{LocalStore, PullBatch},
    sync,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;

#[cfg(test)]
const HTTP_PULL_RESPONSE_MAX_BYTES: u64 = 8 * 1024;
#[cfg(not(test))]
const HTTP_PULL_RESPONSE_MAX_BYTES: u64 = 32 * 1024 * 1024;
const HTTP_PUSH_RESPONSE_MAX_BYTES: u64 = 64 * 1024;

pub struct ServeConfig {
    pub token: Option<String>,
}

pub struct SyncConfig {
    pub token: Option<String>,
    pub allow_insecure_http: bool,
}

pub fn serve(bind: &str, db_path: &str, cfg: ServeConfig) -> Result<()> {
    let token = normalize_configured_token(cfg.token, "HTTP sync token")?;
    let store = LocalStore::open(db_path)?;
    serve_http(bind, store, ServeConfig { token })
}

pub fn sync(
    peers: &[String],
    db_path: &str,
    push: bool,
    local_device_id: Option<&str>,
    cfg: SyncConfig,
) -> Result<()> {
    if peers.is_empty() {
        anyhow::bail!("no peers provided");
    }
    if push && local_device_id.is_none() {
        anyhow::bail!("local_device_id required for push");
    }

    for peer in peers {
        validate_http_sync_peer_url(peer, cfg.allow_insecure_http)?;
    }

    let token = normalize_configured_token(cfg.token, "HTTP sync token")?;
    let store = LocalStore::open(db_path)?;
    let mut progress = sync::SyncRunProgress::new(push);
    let mut last_err: Option<anyhow::Error> = None;
    for peer in peers {
        // peer_id는 우선 URL 문자열을 그대로 사용한다.
        match sync_pull_http_peer(&store, peer, 1000, token.as_deref())
            .with_context(|| format!("pull peer: {peer}"))
        {
            Ok(_) => progress.mark_pull_ok(),
            Err(err) => {
                eprintln!("warn: http pull failed: {peer}: {err:#}");
                last_err = Some(err);
            }
        }

        if push {
            let pending_push = match count_pending_http_push_entries(&store, peer, local_device_id)
            {
                Ok(count) => count,
                Err(err) => {
                    eprintln!("warn: http push preflight failed: {peer}: {err:#}");
                    last_err = Some(err);
                    continue;
                }
            };
            let push_needed = pending_push > 0;
            progress.note_push_needed(push_needed);

            match sync_push_http_peer(&store, peer, 1000, local_device_id, token.as_deref())
                .with_context(|| format!("push peer: {peer}"))
            {
                Ok(_) => progress.mark_push_ok(push_needed),
                Err(err) => {
                    eprintln!("warn: http push failed: {peer}: {err:#}");
                    last_err = Some(err);
                }
            }
        }
    }
    if progress.is_success() {
        Ok(())
    } else {
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("http sync failed")))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct EntriesResponse {
    entries: Vec<Entry>,
    next_cursor: Option<i64>,
    #[serde(default)]
    deletions: Vec<EntryDeletion>,
    #[serde(default)]
    next_delete_cursor: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PushResponse {
    ok: bool,
    inserted: usize,
    ignored: usize,
    deletion_inserted: usize,
    deletion_ignored: usize,
    deletion_deleted: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum EntriesRequest {
    Array(Vec<Entry>),
    Object {
        entries: Vec<Entry>,
        #[serde(default)]
        deletions: Vec<EntryDeletion>,
    },
}

fn serve_http(bind: &str, store: LocalStore, cfg: ServeConfig) -> Result<()> {
    let server =
        tiny_http::Server::http(bind).map_err(|e| anyhow::anyhow!("listen {bind}: {e}"))?;
    let token = cfg.token;

    for mut req in server.incoming_requests() {
        let res = route_http_request(&store, token.as_deref(), &mut req)
            .unwrap_or_else(|err| respond_text(500, &format!("error: {err:#}\n")));
        let _ = req.respond(res);
    }

    Ok(())
}

fn sync_pull_http_peer(
    local: &LocalStore,
    peer_base_url: &str,
    limit: usize,
    token: Option<&str>,
) -> Result<sync::PullStats> {
    let peer_key = normalize_peer_base_url(peer_base_url)?;
    sync::sync_pull_from_peer(local, &peer_key, limit, |cursor, delete_cursor, limit| {
        http_pull_batch(&peer_key, cursor, delete_cursor, limit, token)
    })
}

fn sync_push_http_peer(
    local: &LocalStore,
    peer_base_url: &str,
    limit: usize,
    local_device_id: Option<&str>,
    token: Option<&str>,
) -> Result<usize> {
    let peer_key = normalize_peer_base_url(peer_base_url)?;
    sync::sync_push_to_peer(
        local,
        &peer_key,
        limit,
        local_device_id,
        |entries, deletions| http_push_batch(&peer_key, entries, deletions, token),
    )
}

fn count_pending_http_push_entries(
    local: &LocalStore,
    peer_base_url: &str,
    local_device_id: Option<&str>,
) -> Result<usize> {
    let peer_key = normalize_peer_base_url(peer_base_url)?;
    local.count_pending_push_items(&peer_key, local_device_id)
}

fn normalize_peer_base_url(value: &str) -> Result<String> {
    let v = value.trim().trim_end_matches('/');
    if v.is_empty() {
        anyhow::bail!("peer url is empty");
    }
    Ok(v.to_string())
}

fn validate_http_sync_peer_url(value: &str, allow_insecure_http: bool) -> Result<()> {
    let value = value.trim();
    let uri: ureq::http::Uri = value.parse().context("parse HTTP sync peer URL")?;
    let scheme = uri
        .scheme_str()
        .context("HTTP sync peer URL must include http:// or https://")?;
    let host = uri
        .host()
        .context("HTTP sync peer URL must include a host")?;

    if scheme.eq_ignore_ascii_case("https") {
        return Ok(());
    }
    if !scheme.eq_ignore_ascii_case("http") {
        anyhow::bail!("unsupported HTTP sync peer URL scheme: {scheme}");
    }
    if http_host_is_loopback(host) || allow_insecure_http {
        return Ok(());
    }

    anyhow::bail!(
        "refusing plaintext HTTP sync peer outside loopback: {host}; use HTTPS or pass --allow-insecure-http"
    )
}

fn http_host_is_loopback(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("localhost.")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|addr| addr.is_loopback())
}

fn normalize_configured_token(token: Option<String>, label: &str) -> Result<Option<String>> {
    let Some(token) = token else {
        return Ok(None);
    };

    let token = token.trim();
    crate::tracker::validate_tracker_token_value(token, label)?;

    Ok(Some(token.to_string()))
}

fn http_pull_batch(
    peer_base_url: &str,
    cursor: i64,
    delete_cursor: i64,
    limit: usize,
    token: Option<&str>,
) -> Result<PullBatch> {
    let url = format!(
        "{}/api/v1/entries?cursor={}&delete_cursor={}&limit={}",
        peer_base_url.trim_end_matches('/'),
        cursor,
        delete_cursor,
        limit
    );

    let resp = crate::http_retry::request_with_retry(
        crate::http_retry::RetryPolicy::transport(),
        |agent| {
            let mut req = agent.get(&url);
            if let Some(token) = token {
                req = req.header("Authorization", format!("Bearer {}", token.trim()));
            }
            req.call()
        },
    )
    .with_context(|| format!("GET {url}"))?;
    let mut resp = resp;
    let body = resp
        .body_mut()
        .with_config()
        .limit(HTTP_PULL_RESPONSE_MAX_BYTES)
        .read_to_string()
        .context("read pull response body")?;
    let parsed: EntriesResponse =
        serde_json::from_str(&body).context("parse entries response json")?;

    Ok(PullBatch {
        entries: parsed.entries,
        next_cursor: parsed.next_cursor,
        deletions: parsed.deletions,
        next_delete_cursor: parsed.next_delete_cursor,
    })
}

fn http_push_batch(
    peer_base_url: &str,
    entries: Vec<Entry>,
    deletions: Vec<EntryDeletion>,
    token: Option<&str>,
) -> Result<()> {
    let url = format!("{}/api/v1/entries", peer_base_url.trim_end_matches('/'));

    let body = serde_json::to_vec(&EntriesRequest::Object { entries, deletions })
        .context("serialize entries json")?;
    let resp = crate::http_retry::request_with_retry(
        crate::http_retry::RetryPolicy::transport(),
        |agent| {
            let mut req = agent.post(&url).header("Content-Type", "application/json");
            if let Some(token) = token {
                req = req.header("Authorization", format!("Bearer {}", token.trim()));
            }
            req.send(&body)
        },
    )
    .with_context(|| format!("POST {url}"))?;
    let mut resp = resp;
    let _ = resp
        .body_mut()
        .with_config()
        .limit(HTTP_PUSH_RESPONSE_MAX_BYTES)
        .read_to_string()
        .context("read push response body")?;
    Ok(())
}

fn route_http_request(
    store: &LocalStore,
    token: Option<&str>,
    req: &mut tiny_http::Request,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let url = req.url().to_string();
    let method = req.method().as_str();

    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url.as_str(), None),
    };

    match (method, path) {
        ("GET", "/api/v1/ping") => Ok(respond_text(200, "ok\n")),
        ("GET", "/api/v1/entries") => {
            if !is_authorized(req, token) {
                return Ok(respond_text(401, "unauthorized\n"));
            }
            let (cursor, delete_cursor, limit) = parse_cursor_limit(query)?;
            let batch = store.pull_sync_batch(cursor, delete_cursor, limit)?;
            respond_json(
                200,
                &EntriesResponse {
                    entries: batch.entries,
                    next_cursor: batch.next_cursor,
                    deletions: batch.deletions,
                    next_delete_cursor: batch.next_delete_cursor,
                },
            )
        }
        ("POST", "/api/v1/entries") => {
            if !is_authorized(req, token) {
                return Ok(respond_text(401, "unauthorized\n"));
            }
            let mut buf = Vec::new();
            let max = max_request_body_bytes();
            req.as_reader()
                .take((max as u64).saturating_add(1))
                .read_to_end(&mut buf)
                .context("read request body")?;
            if buf.len() > max {
                return Ok(respond_text(413, "payload too large\n"));
            }

            let req_body: EntriesRequest =
                serde_json::from_slice(&buf).context("parse entries request json")?;
            let (entries, deletions) = match req_body {
                EntriesRequest::Array(entries) => (entries, Vec::new()),
                EntriesRequest::Object { entries, deletions } => (entries, deletions),
            };
            let stats = store.insert_entries_with_stats(&entries)?;
            let deletion_stats = store.apply_entry_deletions_with_stats(&deletions)?;
            respond_json(
                200,
                &PushResponse {
                    ok: true,
                    inserted: stats.inserted,
                    ignored: stats.ignored,
                    deletion_inserted: deletion_stats.inserted,
                    deletion_ignored: deletion_stats.ignored,
                    deletion_deleted: deletion_stats.deleted,
                },
            )
        }
        _ => Ok(respond_text(404, "not found\n")),
    }
}

fn is_authorized(req: &tiny_http::Request, token: Option<&str>) -> bool {
    let Some(token) = token else {
        return true;
    };
    let token = token.trim();
    if token.is_empty() {
        return false;
    }

    if let Some(value) = header_value(req, "Authorization")
        && let Some(rest) = value.strip_prefix("Bearer ")
    {
        return crate::tracker::token_matches(rest, token);
    }

    if let Some(value) = header_value(req, "X-Rustory-Token") {
        return crate::tracker::token_matches(&value, token);
    }

    false
}

fn header_value(req: &tiny_http::Request, name: &'static str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

#[cfg(test)]
fn max_request_body_bytes() -> usize {
    8 * 1024
}

#[cfg(not(test))]
fn max_request_body_bytes() -> usize {
    16 * 1024 * 1024
}

fn parse_cursor_limit(query: Option<&str>) -> Result<(i64, i64, usize)> {
    let mut cursor: i64 = 0;
    let mut delete_cursor: i64 = 0;
    let mut limit: usize = 1000;
    if let Some(query) = query {
        for part in query.split('&') {
            let Some((k, v)) = part.split_once('=') else {
                continue;
            };
            match k {
                "cursor" => cursor = v.parse().context("parse cursor")?,
                "delete_cursor" => delete_cursor = v.parse().context("parse delete cursor")?,
                "limit" => limit = v.parse().context("parse limit")?,
                _ => {}
            }
        }
    }
    Ok((
        cursor,
        delete_cursor,
        limit.clamp(1, crate::sync::SERVER_SYNC_PULL_LIMIT_MAX),
    ))
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;
    use time::OffsetDateTime;

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

    fn start_test_server(db_path: String) -> TestServer {
        start_test_server_with_token(db_path, None)
    }

    fn start_test_server_with_token(db_path: String, token: Option<String>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let bind = format!("127.0.0.1:{}", addr.port());
        let base_url = format!("http://{}", bind);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown2 = shutdown.clone();
        let token2 = token.clone();

        let join = thread::spawn(move || {
            let store = LocalStore::open(&db_path).unwrap();
            let server = tiny_http::Server::http(&bind).unwrap();
            while !shutdown2.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(mut req)) => {
                        let res = route_http_request(&store, token2.as_deref(), &mut req)
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
    fn parse_cursor_limit_clamps_remote_limit_before_storage() {
        let (cursor, delete_cursor, limit) =
            parse_cursor_limit(Some("cursor=7&delete_cursor=9&limit=999999")).unwrap();
        assert_eq!(cursor, 7);
        assert_eq!(delete_cursor, 9);
        assert_eq!(limit, crate::sync::SERVER_SYNC_PULL_LIMIT_MAX);

        let (_, _, limit) = parse_cursor_limit(Some("limit=0")).unwrap();
        assert_eq!(limit, 1);
    }

    #[test]
    fn http_sync_transport_guard_allows_https_and_loopback_only_by_default() {
        for allowed in [
            "https://history.example.test",
            "http://localhost:8844",
            "http://localhost.:8844",
            "http://127.0.0.1:8844",
            "http://[::1]:8844",
        ] {
            validate_http_sync_peer_url(allowed, false).unwrap();
        }

        for rejected in [
            "http://192.168.1.10:8844",
            "http://history.example.test",
            "ftp://history.example.test",
            "history.example.test",
        ] {
            assert!(validate_http_sync_peer_url(rejected, false).is_err());
        }

        validate_http_sync_peer_url("http://192.168.1.10:8844", true).unwrap();
    }

    #[test]
    fn http_sync_rejects_remote_plaintext_before_opening_database() {
        let dir = tempdir().unwrap();
        let local_db = dir.path().join("local.db");
        let peers = vec!["http://192.168.1.10:8844".to_string()];

        let err = sync(
            &peers,
            local_db.to_str().unwrap(),
            false,
            None,
            SyncConfig {
                token: None,
                allow_insecure_http: false,
            },
        )
        .unwrap_err();

        assert!(format!("{err:#}").contains("refusing plaintext HTTP sync peer"));
        assert!(!local_db.exists());
    }

    fn entry(entry_id: &str, ts: i64, cmd: &str) -> Entry {
        Entry {
            entry_id: entry_id.to_string(),
            device_id: "dev1".to_string(),
            user_id: "user1".to_string(),
            ts: OffsetDateTime::from_unix_timestamp(ts).unwrap(),
            cmd: cmd.to_string(),
            cwd: "/tmp".to_string(),
            exit_code: 0,
            duration_ms: 12,
            shell: "zsh".to_string(),
            hostname: "host".to_string(),
            version: crate::build_info::VERSION.to_string(),
        }
    }

    #[test]
    fn http_server_and_sync_client_end_to_end() {
        let dir = tempdir().unwrap();
        let remote_db = dir.path().join("remote.db");
        let local_db = dir.path().join("local.db");

        let remote = LocalStore::open(remote_db.to_str().unwrap()).unwrap();
        let mut r1 = entry("id-1", 1, "echo 1");
        r1.device_id = "dev-remote".to_string();
        let mut r2 = entry("id-2", 2, "echo 2");
        r2.device_id = "dev-remote".to_string();
        remote.insert_entries(&[r1, r2]).unwrap();

        let server = start_test_server(remote_db.to_str().unwrap().to_string());

        let local = LocalStore::open(local_db.to_str().unwrap()).unwrap();
        let pulled = sync_pull_http_peer(&local, &server.base_url, 1, None).unwrap();

        assert_eq!(pulled.received, 2);
        assert_eq!(pulled.inserted, 2);
        assert_eq!(pulled.ignored, 0);
        assert_eq!(local.list_recent(10).unwrap().len(), 2);
        assert_eq!(local.get_last_cursor(&server.base_url).unwrap(), 2);

        let mut local_entry = entry("id-3", 3, "echo 3");
        local_entry.device_id = "dev-local".to_string();
        local.insert_entries(&[local_entry]).unwrap();

        let pushed =
            sync_push_http_peer(&local, &server.base_url, 100, Some("dev-local"), None).unwrap();
        assert_eq!(pushed, 1);

        let got = remote.list_recent(10).unwrap();
        assert_eq!(got.len(), 3);
        assert!(got.iter().any(|e| e.entry_id == "id-3"));

        server.shutdown();
    }

    #[test]
    fn http_pull_adapts_when_response_exceeds_body_limit() {
        let dir = tempdir().unwrap();
        let remote_db = dir.path().join("remote.db");
        let local_db = dir.path().join("local.db");

        let remote = LocalStore::open(remote_db.to_str().unwrap()).unwrap();
        let mut first = entry("id-large-1", 1, &"a".repeat(5 * 1024));
        first.device_id = "dev-remote".to_string();
        let mut second = entry("id-large-2", 2, &"b".repeat(5 * 1024));
        second.device_id = "dev-remote".to_string();
        remote.insert_entries(&[first, second]).unwrap();

        let server = start_test_server(remote_db.to_str().unwrap().to_string());
        let local = LocalStore::open(local_db.to_str().unwrap()).unwrap();

        let pulled = sync_pull_http_peer(&local, &server.base_url, 2, None).unwrap();

        assert_eq!(pulled.received, 2);
        assert_eq!(pulled.inserted, 2);
        assert_eq!(local.list_recent(10).unwrap().len(), 2);
        server.shutdown();
    }

    #[test]
    fn http_sync_normalizes_peer_url_key() {
        let dir = tempdir().unwrap();
        let remote_db = dir.path().join("remote.db");
        let local_db = dir.path().join("local.db");

        let remote = LocalStore::open(remote_db.to_str().unwrap()).unwrap();
        let mut r1 = entry("id-1", 1, "echo 1");
        r1.device_id = "dev-remote".to_string();
        remote.insert_entries(&[r1]).unwrap();

        let server = start_test_server(remote_db.to_str().unwrap().to_string());

        let local = LocalStore::open(local_db.to_str().unwrap()).unwrap();
        let peer_with_slash = format!("{}/", server.base_url);
        let pulled = sync_pull_http_peer(&local, &peer_with_slash, 100, None).unwrap();
        assert_eq!(pulled.received, 1);
        assert_eq!(pulled.inserted, 1);
        assert_eq!(pulled.ignored, 0);

        // cursor는 normalize된 key(끝의 / 제거)로 저장된다.
        assert_eq!(local.get_last_cursor(&server.base_url).unwrap(), 1);
        assert_eq!(local.get_last_cursor(&peer_with_slash).unwrap(), 0);

        server.shutdown();
    }

    #[test]
    fn http_entries_require_token_when_configured() {
        let dir = tempdir().unwrap();
        let remote_db = dir.path().join("remote.db");
        let remote = LocalStore::open(remote_db.to_str().unwrap()).unwrap();
        remote
            .insert_entries(&[entry("id-1", 1, "echo 1")])
            .unwrap();

        let server = start_test_server_with_token(
            remote_db.to_str().unwrap().to_string(),
            Some("sync-secret".to_string()),
        );

        let url = format!("{}/api/v1/entries?cursor=0&limit=1", server.base_url);
        let err = ureq::get(&url).call().unwrap_err();
        let ureq::Error::StatusCode(status) = err else {
            panic!("expected status error");
        };
        assert_eq!(status, 401);

        let resp = ureq::get(&url)
            .header("Authorization", "Bearer sync-secret")
            .call()
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);

        server.shutdown();
    }

    #[test]
    fn http_sync_rejects_blank_configured_token() {
        let err =
            normalize_configured_token(Some(" \t ".to_string()), "HTTP sync token").unwrap_err();
        assert!(format!("{err:#}").contains("HTTP sync token must not be empty"));

        let dir = tempdir().unwrap();
        let remote_db = dir.path().join("remote.db");
        let local_db = dir.path().join("local.db");
        let remote = LocalStore::open(remote_db.to_str().unwrap()).unwrap();
        remote
            .insert_entries(&[entry("id-1", 1, "echo 1")])
            .unwrap();

        let server = start_test_server_with_token(
            remote_db.to_str().unwrap().to_string(),
            Some("   ".into()),
        );

        let url = format!("{}/api/v1/entries?cursor=0&limit=1", server.base_url);
        let err = ureq::get(&url).call().unwrap_err();
        let ureq::Error::StatusCode(status) = err else {
            panic!("expected status error");
        };
        assert_eq!(status, 401);

        let peers = vec![server.base_url.clone()];
        let err = sync(
            &peers,
            local_db.to_str().unwrap(),
            false,
            None,
            SyncConfig {
                token: Some("   ".into()),
                allow_insecure_http: false,
            },
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("HTTP sync token must not be empty"));

        server.shutdown();
    }

    #[test]
    fn http_push_returns_insert_stats() {
        let dir = tempdir().unwrap();
        let remote_db = dir.path().join("remote.db");
        let server = start_test_server(remote_db.to_str().unwrap().to_string());

        let e1 = entry("id-1", 1, "echo 1");
        let e2 = entry("id-2", 2, "echo 2");
        let body = serde_json::to_vec(&[e1.clone(), e2.clone(), e1]).unwrap();

        let url = format!("{}/api/v1/entries", server.base_url);
        let mut resp = ureq::post(&url)
            .header("Content-Type", "application/json")
            .send(&body)
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);

        let text = resp.body_mut().read_to_string().unwrap();
        let parsed: PushResponse = serde_json::from_str(&text).unwrap();
        assert!(parsed.ok);
        assert_eq!(parsed.inserted, 2);
        assert_eq!(parsed.ignored, 1);

        server.shutdown();
    }

    #[test]
    fn http_sync_with_push_fails_when_pending_upload_cannot_be_sent() {
        let dir = tempdir().unwrap();
        let local_db = dir.path().join("local.db");
        let local = LocalStore::open(local_db.to_str().unwrap()).unwrap();

        let mut local_entry = entry("id-local", 1, "echo local");
        local_entry.device_id = "dev-local".to_string();
        local.insert_entries(&[local_entry]).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let bind = format!("127.0.0.1:{}", addr.port());
        let base_url = format!("http://{}", bind);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown2 = shutdown.clone();

        let join = thread::spawn(move || {
            let server = tiny_http::Server::http(&bind).unwrap();
            while !shutdown2.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(req)) => {
                        let path = req.url().split('?').next().unwrap_or(req.url());
                        let res = match (req.method().as_str(), path) {
                            ("GET", "/api/v1/ping") => respond_text(200, "ok\n"),
                            ("GET", "/api/v1/entries") => respond_json(
                                200,
                                &EntriesResponse {
                                    entries: vec![],
                                    next_cursor: None,
                                    deletions: vec![],
                                    next_delete_cursor: None,
                                },
                            )
                            .unwrap(),
                            ("POST", "/api/v1/entries") => respond_text(500, "push failed\n"),
                            _ => respond_text(404, "not found\n"),
                        };
                        let _ = req.respond(res);
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });

        for _ in 0..50 {
            let url = format!("{}/api/v1/ping", base_url);
            if ureq::get(&url).call().is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        let peers = vec![base_url];
        let err = sync(
            &peers,
            local_db.to_str().unwrap(),
            true,
            Some("dev-local"),
            SyncConfig {
                token: None,
                allow_insecure_http: false,
            },
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("push peer"));

        shutdown.store(true, Ordering::SeqCst);
        let _ = join.join();
    }
}
