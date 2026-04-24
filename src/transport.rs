use crate::{core::Entry, storage::LocalStore, sync};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub fn serve(bind: &str, db_path: &str) -> Result<()> {
    let store = LocalStore::open(db_path)?;
    serve_http(bind, store)
}

pub fn sync(
    peers: &[String],
    db_path: &str,
    push: bool,
    local_device_id: Option<&str>,
) -> Result<()> {
    if peers.is_empty() {
        anyhow::bail!("no peers provided");
    }
    if push && local_device_id.is_none() {
        anyhow::bail!("local_device_id required for push");
    }

    let store = LocalStore::open(db_path)?;
    let mut any_ok = false;
    let mut last_err: Option<anyhow::Error> = None;
    for peer in peers {
        // peer_id는 우선 URL 문자열을 그대로 사용한다.
        match sync_pull_http_peer(&store, peer, 1000).with_context(|| format!("pull peer: {peer}"))
        {
            Ok(_) => any_ok = true,
            Err(err) => {
                eprintln!("warn: http pull failed: {peer}: {err:#}");
                last_err = Some(err);
            }
        }

        if push {
            match sync_push_http_peer(&store, peer, 1000, local_device_id)
                .with_context(|| format!("push peer: {peer}"))
            {
                Ok(pushed) => {
                    if pushed > 0 {
                        any_ok = true;
                    }
                }
                Err(err) => {
                    eprintln!("warn: http push failed: {peer}: {err:#}");
                    last_err = Some(err);
                }
            }
        }
    }
    if any_ok {
        Ok(())
    } else {
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("http sync failed")))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct EntriesResponse {
    entries: Vec<Entry>,
    next_cursor: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EntriesRequest {
    Array(Vec<Entry>),
    Object { entries: Vec<Entry> },
}

fn serve_http(bind: &str, store: LocalStore) -> Result<()> {
    let server =
        tiny_http::Server::http(bind).map_err(|e| anyhow::anyhow!("listen {bind}: {e}"))?;

    for mut req in server.incoming_requests() {
        let res = route_http_request(&store, &mut req)
            .unwrap_or_else(|err| respond_text(500, &format!("error: {err:#}\n")));
        let _ = req.respond(res);
    }

    Ok(())
}

fn sync_pull_http_peer(local: &LocalStore, peer_base_url: &str, limit: usize) -> Result<usize> {
    sync::sync_pull_from_peer(local, peer_base_url, limit, |cursor, limit| {
        http_pull_batch(peer_base_url, cursor, limit)
    })
}

fn sync_push_http_peer(
    local: &LocalStore,
    peer_base_url: &str,
    limit: usize,
    local_device_id: Option<&str>,
) -> Result<usize> {
    sync::sync_push_to_peer(local, peer_base_url, limit, local_device_id, |entries| {
        http_push_batch(peer_base_url, entries)
    })
}

fn http_pull_batch(
    peer_base_url: &str,
    cursor: i64,
    limit: usize,
) -> Result<crate::storage::PullBatch> {
    let url = format!(
        "{}/api/v1/entries?cursor={}&limit={}",
        peer_base_url.trim_end_matches('/'),
        cursor,
        limit
    );

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(30))
        .build();

    let resp = agent
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let body = resp.into_string().context("read response body")?;
    let parsed: EntriesResponse =
        serde_json::from_str(&body).context("parse entries response json")?;

    Ok(crate::storage::PullBatch {
        entries: parsed.entries,
        next_cursor: parsed.next_cursor,
    })
}

fn http_push_batch(peer_base_url: &str, entries: Vec<Entry>) -> Result<()> {
    let url = format!("{}/api/v1/entries", peer_base_url.trim_end_matches('/'));

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(30))
        .build();

    let body = serde_json::to_vec(&entries).context("serialize entries json")?;
    let resp = agent
        .post(&url)
        .set("Content-Type", "application/json")
        .send_bytes(&body)
        .with_context(|| format!("POST {url}"))?;
    let _ = resp.into_string().context("read response body")?;
    Ok(())
}

fn route_http_request(
    store: &LocalStore,
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
            let (cursor, limit) = parse_cursor_limit(query)?;
            let batch = store.pull_since_cursor(cursor, limit)?;
            respond_json(
                200,
                &EntriesResponse {
                    entries: batch.entries,
                    next_cursor: batch.next_cursor,
                },
            )
        }
        ("POST", "/api/v1/entries") => {
            let mut buf = Vec::new();
            req.as_reader()
                .read_to_end(&mut buf)
                .context("read request body")?;

            let req_body: EntriesRequest =
                serde_json::from_slice(&buf).context("parse entries request json")?;
            let entries = match req_body {
                EntriesRequest::Array(entries) => entries,
                EntriesRequest::Object { entries } => entries,
            };
            store.insert_entries(&entries)?;

            Ok(respond_text(200, "ok\n"))
        }
        _ => Ok(respond_text(404, "not found\n")),
    }
}

fn parse_cursor_limit(query: Option<&str>) -> Result<(i64, usize)> {
    let mut cursor: i64 = 0;
    let mut limit: usize = 1000;
    if let Some(query) = query {
        for part in query.split('&') {
            let Some((k, v)) = part.split_once('=') else {
                continue;
            };
            match k {
                "cursor" => cursor = v.parse().context("parse cursor")?,
                "limit" => limit = v.parse().context("parse limit")?,
                _ => {}
            }
        }
    }
    Ok((cursor, limit))
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
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let bind = format!("127.0.0.1:{}", addr.port());
        let base_url = format!("http://{}", bind);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown2 = shutdown.clone();

        let join = thread::spawn(move || {
            let store = LocalStore::open(&db_path).unwrap();
            let server = tiny_http::Server::http(&bind).unwrap();
            while !shutdown2.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(mut req)) => {
                        let res = route_http_request(&store, &mut req)
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
            version: "0.1.0".to_string(),
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
        let pulled = sync_pull_http_peer(&local, &server.base_url, 1).unwrap();

        assert_eq!(pulled, 2);
        assert_eq!(local.list_recent(10).unwrap().len(), 2);
        assert_eq!(local.get_last_cursor(&server.base_url).unwrap(), 2);

        let mut local_entry = entry("id-3", 3, "echo 3");
        local_entry.device_id = "dev-local".to_string();
        local.insert_entries(&[local_entry]).unwrap();

        let pushed = sync_push_http_peer(&local, &server.base_url, 100, Some("dev-local")).unwrap();
        assert_eq!(pushed, 1);

        let got = remote.list_recent(10).unwrap();
        assert_eq!(got.len(), 3);
        assert!(got.iter().any(|e| e.entry_id == "id-3"));

        server.shutdown();
    }
}
