use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::storage;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct SyncStatusPeerReport {
    pub(crate) peer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) peer_device_id: Option<String>,
    pub(crate) pull_cursor: i64,
    pub(crate) push_cursor: i64,
    pub(crate) outbound_push_pending: usize,
    pub(crate) pending_push: usize,
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
) -> Result<SyncStatusReport> {
    let local_head = store.latest_ingest_seq()?;
    let peer_last_seen = store.list_peer_book_last_seen_map()?;
    let peer_device_ids = store
        .list_peer_book(None, 0, 1000)?
        .into_iter()
        .filter_map(|peer| peer.device_id.map(|device_id| (peer.peer_id, device_id)))
        .collect::<HashMap<_, _>>();
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
        let peer_device_id = peer_device_ids.get(&peer_id).cloned();
        if sync_device_id_matches(peer_device_id.as_deref(), local_device_id) {
            continue;
        }
        let pending_push = store.count_pending_push_entries(&peer_id, Some(local_device_id))?;
        let last_seen_unix = peer_last_seen.get(&peer_id).copied();
        let last_seen_age_sec = compute_last_seen_age_sec(now_unix, last_seen_unix);
        peers.push(SyncStatusPeerReport {
            peer_device_id,
            peer_id,
            pull_cursor: status.last_cursor,
            push_cursor: status.last_pushed_seq,
            outbound_push_pending: pending_push,
            pending_push,
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
) -> Result<SyncStatusReport> {
    let tracker_status =
        trackers.map(|trackers| build_tracker_status_report(trackers, tracker_token));
    build_sync_status_report(
        store,
        local_device_id,
        local_peer_id,
        peer_filter,
        tracker_status,
    )
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
