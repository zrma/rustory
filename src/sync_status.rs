use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::storage;
use crate::tracker;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct SyncStatusPeerReport {
    pub(crate) peer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) peer_device_id: Option<String>,
    pub(crate) pull_cursor: i64,
    pub(crate) pull_delete_cursor: i64,
    pub(crate) push_cursor: i64,
    pub(crate) push_delete_cursor: i64,
    pub(crate) outbound_push_pending: usize,
    pub(crate) outbound_push_pending_entries: usize,
    pub(crate) outbound_push_pending_deletions: usize,
    pub(crate) pending_push: usize,
    pub(crate) pending_push_entries: usize,
    pub(crate) pending_push_deletions: usize,
    pub(crate) last_seen_unix: Option<i64>,
    pub(crate) last_seen_age_sec: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct SyncStatusTrackerReport {
    pub(crate) base_url: String,
    pub(crate) reachable: bool,
    pub(crate) latency_ms: Option<u64>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct SyncStatusReport {
    pub(crate) local_head: i64,
    pub(crate) local_device_id: String,
    pub(crate) peers: Vec<SyncStatusPeerReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tracker_status: Option<Vec<SyncStatusTrackerReport>>,
}

pub(crate) fn compute_last_seen_age_sec(now_unix: i64, last_seen_unix: Option<i64>) -> Option<i64> {
    last_seen_unix.map(|ts| now_unix.saturating_sub(ts).max(0))
}

pub(crate) fn build_sync_status_report(
    store: &storage::LocalStore,
    local_device_id: &str,
    local_peer_id: Option<&str>,
    peer_filter: Option<&str>,
    tracker_status: Option<Vec<SyncStatusTrackerReport>>,
    tracker_peers: &[storage::PeerBookPeer],
) -> Result<SyncStatusReport> {
    let local_head = store.latest_ingest_seq()?;
    let mut peer_metadata = store
        .list_peer_book(None, 0, 1000)?
        .into_iter()
        .map(|peer| (peer.peer_id, (peer.device_id, peer.last_seen_unix)))
        .collect::<HashMap<_, _>>();
    for peer in tracker_peers {
        if local_peer_id == Some(peer.peer_id.as_str()) {
            continue;
        }
        if sync_device_id_matches(peer.device_id.as_deref(), local_device_id) {
            continue;
        }
        match peer_metadata.get(&peer.peer_id) {
            Some((_device_id, last_seen)) if *last_seen > peer.last_seen_unix => {}
            _ => {
                peer_metadata.insert(
                    peer.peer_id.clone(),
                    (peer.device_id.clone(), peer.last_seen_unix),
                );
            }
        }
    }
    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    let mut statuses = store.list_peer_sync_status()?;
    if let Some(peer_id) = peer_filter {
        statuses.retain(|status| status.peer_id == peer_id);
    }

    let mut peers = Vec::with_capacity(statuses.len());
    for status in statuses {
        let peer_id = status.peer_id;
        if local_peer_id == Some(peer_id.as_str()) {
            continue;
        }
        let (peer_device_id, last_seen_unix) = peer_metadata
            .get(&peer_id)
            .map(|(device_id, last_seen_unix)| (device_id.clone(), Some(*last_seen_unix)))
            .unwrap_or((None, None));
        if sync_device_id_matches(peer_device_id.as_deref(), local_device_id) {
            continue;
        }
        let pending_push_entries =
            store.count_pending_push_entries(&peer_id, Some(local_device_id))?;
        let pending_push_deletions =
            store.count_pending_push_deletions(&peer_id, Some(local_device_id))?;
        let pending_push = pending_push_entries + pending_push_deletions;
        let pull_delete_cursor = store.get_last_delete_cursor(&peer_id)?;
        let push_delete_cursor = store.get_last_pushed_delete_seq(&peer_id)?;
        let last_seen_age_sec = compute_last_seen_age_sec(now_unix, last_seen_unix);
        peers.push(SyncStatusPeerReport {
            peer_device_id,
            peer_id,
            pull_cursor: status.last_cursor,
            pull_delete_cursor,
            push_cursor: status.last_pushed_seq,
            push_delete_cursor,
            outbound_push_pending: pending_push,
            outbound_push_pending_entries: pending_push_entries,
            outbound_push_pending_deletions: pending_push_deletions,
            pending_push,
            pending_push_entries,
            pending_push_deletions,
            last_seen_unix,
            last_seen_age_sec,
        });
    }

    Ok(SyncStatusReport {
        local_head,
        local_device_id: local_device_id.to_string(),
        peers,
        tracker_status,
    })
}

pub(crate) fn build_sync_status_report_for_cli(
    store: &storage::LocalStore,
    local_device_id: &str,
    local_peer_id: Option<&str>,
    peer_filter: Option<&str>,
    trackers: Option<&[String]>,
    tracker_token: Option<&str>,
    user_id: Option<&str>,
) -> Result<SyncStatusReport> {
    let tracker_status =
        trackers.map(|trackers| build_tracker_status_report(trackers, tracker_token));
    let tracker_peers = trackers
        .map(|trackers| list_tracker_peers_for_status(trackers, tracker_token, user_id))
        .unwrap_or_default();
    build_sync_status_report(
        store,
        local_device_id,
        local_peer_id,
        peer_filter,
        tracker_status,
        &tracker_peers,
    )
}

pub(crate) fn list_tracker_peers_for_status(
    trackers: &[String],
    tracker_token: Option<&str>,
    user_id: Option<&str>,
) -> Vec<storage::PeerBookPeer> {
    let mut peers = Vec::new();
    for base_url in trackers {
        let client = tracker::TrackerClient::new(
            base_url.clone(),
            tracker_token.map(std::string::ToString::to_string),
        );
        let list = match client.list(user_id) {
            Ok(list) => list,
            Err(err) => {
                eprintln!("warn: tracker list failed for sync-status: {base_url}: {err:#}");
                continue;
            }
        };
        peers.extend(list.peers.into_iter().map(|peer| storage::PeerBookPeer {
            peer_id: peer.peer_id,
            addrs: peer.addrs,
            user_id: peer.meta.as_ref().and_then(|meta| meta.user_id.clone()),
            device_id: peer.meta.as_ref().and_then(|meta| meta.device_id.clone()),
            last_seen_unix: peer.last_seen_unix,
        }));
    }
    peers
}

fn sync_device_id_matches(peer_device_id: Option<&str>, local_device_id: &str) -> bool {
    peer_device_id
        .map(|device_id| device_id.trim() == local_device_id.trim())
        .unwrap_or(false)
}

pub(crate) fn build_tracker_status_report(
    trackers: &[String],
    tracker_token: Option<&str>,
) -> Vec<SyncStatusTrackerReport> {
    trackers
        .iter()
        .map(|base_url| match tracker_ping(base_url, tracker_token) {
            Ok(latency_ms) => SyncStatusTrackerReport {
                base_url: base_url.clone(),
                reachable: true,
                latency_ms: Some(latency_ms),
                error: None,
            },
            Err(err) => SyncStatusTrackerReport {
                base_url: base_url.clone(),
                reachable: false,
                latency_ms: None,
                error: Some(err),
            },
        })
        .collect()
}

pub(crate) fn tracker_ping(
    base_url: &str,
    token: Option<&str>,
) -> std::result::Result<u64, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(1)))
        .timeout_send_request(Some(Duration::from_secs(1)))
        .timeout_send_body(Some(Duration::from_secs(1)))
        .timeout_recv_response(Some(Duration::from_secs(1)))
        .timeout_recv_body(Some(Duration::from_secs(1)))
        .build()
        .into();

    let url = format!("{}/api/v1/ping", base_url.trim_end_matches('/'));
    let mut req = agent.get(&url);
    if let Some(token) = token {
        req = req.header("Authorization", format!("Bearer {}", token.trim()));
    }

    let started = Instant::now();
    match req.call() {
        Ok(resp) => {
            if resp.status().as_u16() == 200 {
                let elapsed_ms = started.elapsed().as_millis();
                let latency_ms = u64::try_from(elapsed_ms).unwrap_or(u64::MAX);
                Ok(latency_ms)
            } else {
                Err(format!("status {}", resp.status()))
            }
        }
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_status_report_prefers_newer_tracker_peer_metadata() {
        let store = storage::LocalStore::open(":memory:").unwrap();
        store.set_last_cursor("peer-a", 10).unwrap();
        store
            .upsert_peer_book(&storage::PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1111/p2p/peer-a".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some("old-device".to_string()),
                last_seen_unix: 100,
            })
            .unwrap();

        let tracker_peers = vec![storage::PeerBookPeer {
            peer_id: "peer-a".to_string(),
            addrs: vec![
                "/dns4/relay.example/tcp/4001/p2p/relay/p2p-circuit/p2p/peer-a".to_string(),
            ],
            user_id: Some("user1".to_string()),
            device_id: Some("new-device".to_string()),
            last_seen_unix: 200,
        }];

        let report =
            build_sync_status_report(&store, "local-device", None, None, None, &tracker_peers)
                .unwrap();
        let peer = report
            .peers
            .iter()
            .find(|peer| peer.peer_id == "peer-a")
            .unwrap();
        assert_eq!(peer.peer_device_id.as_deref(), Some("new-device"));
        assert_eq!(peer.last_seen_unix, Some(200));
    }

    #[test]
    fn sync_status_report_keeps_local_metadata_when_tracker_record_is_older() {
        let store = storage::LocalStore::open(":memory:").unwrap();
        store.set_last_cursor("peer-a", 10).unwrap();
        store
            .upsert_peer_book(&storage::PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1111/p2p/peer-a".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some("local-newer-device".to_string()),
                last_seen_unix: 300,
            })
            .unwrap();

        let tracker_peers = vec![storage::PeerBookPeer {
            peer_id: "peer-a".to_string(),
            addrs: vec![
                "/dns4/relay.example/tcp/4001/p2p/relay/p2p-circuit/p2p/peer-a".to_string(),
            ],
            user_id: Some("user1".to_string()),
            device_id: Some("tracker-older-device".to_string()),
            last_seen_unix: 200,
        }];

        let report =
            build_sync_status_report(&store, "local-device", None, None, None, &tracker_peers)
                .unwrap();
        let peer = report
            .peers
            .iter()
            .find(|peer| peer.peer_id == "peer-a")
            .unwrap();
        assert_eq!(peer.peer_device_id.as_deref(), Some("local-newer-device"));
        assert_eq!(peer.last_seen_unix, Some(300));
    }
}
