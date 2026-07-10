use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::storage;
use crate::tracker;

const ACTIVE_DUPLICATE_LAST_SEEN_MAX_AGE_SEC: i64 = 5 * 60;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct SyncStatusPeerReport {
    pub(crate) peer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) peer_device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) peer_hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) peer_rr_version: Option<String>,
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
pub(crate) struct SyncStatusWarning {
    pub(crate) code: String,
    pub(crate) message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) peer_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) device_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) hostnames: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncStatusPeerMetadata {
    pub(crate) peer_id: String,
    pub(crate) device_id: Option<String>,
    pub(crate) hostname: Option<String>,
    pub(crate) rr_version: Option<String>,
    pub(crate) last_seen_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PeerMetadata {
    device_id: Option<String>,
    hostname: Option<String>,
    rr_version: Option<String>,
    last_seen_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MembershipPeer {
    peer_id: String,
    device_id: Option<String>,
    hostname: Option<String>,
    last_seen_age_sec: Option<i64>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<SyncStatusWarning>,
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
    tracker_peers: &[SyncStatusPeerMetadata],
) -> Result<SyncStatusReport> {
    let local_head = store.latest_ingest_seq()?;
    let mut peer_metadata = store
        .list_peer_book(None, 0, 1000)?
        .into_iter()
        .map(|peer| {
            (
                peer.peer_id,
                PeerMetadata {
                    device_id: peer.device_id,
                    hostname: None,
                    rr_version: peer.rr_version,
                    last_seen_unix: peer.last_seen_unix,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    for peer in tracker_peers {
        if let Some(existing) = peer_metadata.get_mut(&peer.peer_id)
            && existing.last_seen_unix > peer.last_seen_unix
        {
            if existing.rr_version.is_none() {
                existing.rr_version = peer.rr_version.clone();
            }
            continue;
        }
        let rr_version = peer.rr_version.clone().or_else(|| {
            peer_metadata
                .get(&peer.peer_id)
                .and_then(|existing| existing.rr_version.clone())
        });
        peer_metadata.insert(
            peer.peer_id.clone(),
            PeerMetadata {
                device_id: peer.device_id.clone(),
                hostname: peer.hostname.clone(),
                rr_version,
                last_seen_unix: peer.last_seen_unix,
            },
        );
    }
    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    let membership_peers = build_membership_peers(&peer_metadata, now_unix);
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
        let metadata = peer_metadata.get(&peer_id);
        let peer_device_id = metadata.and_then(|metadata| metadata.device_id.clone());
        let peer_hostname = metadata.and_then(|metadata| metadata.hostname.clone());
        let peer_rr_version = metadata.and_then(|metadata| metadata.rr_version.clone());
        let last_seen_unix = metadata.map(|metadata| metadata.last_seen_unix);
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
            peer_hostname,
            peer_rr_version,
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
    let warnings = build_sync_status_warnings(&membership_peers);

    Ok(SyncStatusReport {
        local_head,
        local_device_id: local_device_id.to_string(),
        peers,
        warnings,
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
) -> Vec<SyncStatusPeerMetadata> {
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
        peers.extend(list.peers.into_iter().map(|peer| SyncStatusPeerMetadata {
            peer_id: peer.peer_id,
            device_id: peer.meta.as_ref().and_then(|meta| meta.device_id.clone()),
            hostname: peer.meta.as_ref().and_then(|meta| meta.hostname.clone()),
            rr_version: peer.meta.as_ref().and_then(|meta| meta.version.clone()),
            last_seen_unix: peer.last_seen_unix,
        }));
    }
    peers
}

fn build_membership_peers(
    peer_metadata: &HashMap<String, PeerMetadata>,
    now_unix: i64,
) -> Vec<MembershipPeer> {
    peer_metadata
        .iter()
        .map(|(peer_id, metadata)| MembershipPeer {
            peer_id: peer_id.clone(),
            device_id: metadata.device_id.clone(),
            hostname: metadata.hostname.clone(),
            last_seen_age_sec: compute_last_seen_age_sec(now_unix, Some(metadata.last_seen_unix)),
        })
        .collect()
}

fn build_sync_status_warnings(peers: &[MembershipPeer]) -> Vec<SyncStatusWarning> {
    let mut warnings = Vec::new();
    warnings.extend(build_active_duplicate_warnings(
        peers,
        "active_duplicate_hostname",
        "hostname",
        |peer| peer.hostname.as_deref(),
    ));
    warnings.extend(build_active_duplicate_warnings(
        peers,
        "active_duplicate_device_id",
        "device_id",
        |peer| peer.device_id.as_deref(),
    ));
    warnings
}

fn build_active_duplicate_warnings<F>(
    peers: &[MembershipPeer],
    code: &str,
    field_label: &str,
    value_of: F,
) -> Vec<SyncStatusWarning>
where
    F: Fn(&MembershipPeer) -> Option<&str>,
{
    let mut groups: HashMap<String, Vec<&MembershipPeer>> = HashMap::new();
    for peer in peers {
        let Some(value) = value_of(peer).and_then(normalize_membership_key) else {
            continue;
        };
        if !is_active_membership_peer(peer) {
            continue;
        }
        groups.entry(value).or_default().push(peer);
    }

    let mut warnings = groups
        .into_iter()
        .filter_map(|(value, group)| {
            if group.len() < 2 {
                return None;
            }
            let mut peer_ids = group
                .iter()
                .map(|peer| peer.peer_id.clone())
                .collect::<Vec<_>>();
            peer_ids.sort();
            peer_ids.dedup();
            let mut device_ids = group
                .iter()
                .filter_map(|peer| peer.device_id.clone())
                .collect::<Vec<_>>();
            device_ids.sort();
            device_ids.dedup();
            let mut hostnames = group
                .iter()
                .filter_map(|peer| peer.hostname.clone())
                .collect::<Vec<_>>();
            hostnames.sort();
            hostnames.dedup();

            Some(SyncStatusWarning {
                code: code.to_string(),
                message: format!(
                    "multiple active peers share {field_label} '{value}'; if this followed uninstall/rejoin, retire the old identity or keep only one active node"
                ),
                peer_ids,
                device_ids,
                hostnames,
            })
        })
        .collect::<Vec<_>>();
    warnings.sort_by(|a, b| a.code.cmp(&b.code).then(a.message.cmp(&b.message)));
    warnings
}

fn normalize_membership_key(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    (!normalized.is_empty() && normalized != "unknown").then_some(normalized)
}

fn is_active_membership_peer(peer: &MembershipPeer) -> bool {
    matches!(
        peer.last_seen_age_sec,
        Some(age) if age <= ACTIVE_DUPLICATE_LAST_SEEN_MAX_AGE_SEC
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
                rr_version: Some("1.0.43".to_string()),
                last_seen_unix: 100,
            })
            .unwrap();

        let tracker_peers = vec![SyncStatusPeerMetadata {
            peer_id: "peer-a".to_string(),
            device_id: Some("new-device".to_string()),
            hostname: Some("new-host".to_string()),
            rr_version: Some("1.0.45".to_string()),
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
        assert_eq!(peer.peer_hostname.as_deref(), Some("new-host"));
        assert_eq!(peer.peer_rr_version.as_deref(), Some("1.0.45"));
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
                rr_version: None,
                last_seen_unix: 300,
            })
            .unwrap();

        let tracker_peers = vec![SyncStatusPeerMetadata {
            peer_id: "peer-a".to_string(),
            device_id: Some("tracker-older-device".to_string()),
            hostname: Some("tracker-older-host".to_string()),
            rr_version: Some("1.0.44".to_string()),
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
        assert_eq!(peer.peer_hostname.as_deref(), None);
        assert_eq!(peer.peer_rr_version.as_deref(), Some("1.0.44"));
        assert_eq!(peer.last_seen_unix, Some(300));
    }

    #[test]
    fn sync_status_report_warns_for_active_duplicate_hostnames() {
        let store = storage::LocalStore::open(":memory:").unwrap();
        store.set_last_cursor("peer-a", 10).unwrap();
        store.set_last_cursor("peer-b", 20).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let tracker_peers = vec![
            SyncStatusPeerMetadata {
                peer_id: "peer-a".to_string(),
                device_id: Some("host-a-old".to_string()),
                hostname: Some("workstation".to_string()),
                rr_version: None,
                last_seen_unix: now - 30,
            },
            SyncStatusPeerMetadata {
                peer_id: "peer-b".to_string(),
                device_id: Some("host-a-new".to_string()),
                hostname: Some("WORKSTATION ".to_string()),
                rr_version: None,
                last_seen_unix: now - 40,
            },
        ];

        let report =
            build_sync_status_report(&store, "local-device", None, None, None, &tracker_peers)
                .unwrap();

        assert_eq!(report.warnings.len(), 1);
        let warning = &report.warnings[0];
        assert_eq!(warning.code, "active_duplicate_hostname");
        assert!(warning.message.contains("workstation"));
        assert_eq!(warning.peer_ids, vec!["peer-a", "peer-b"]);
        assert_eq!(warning.device_ids, vec!["host-a-new", "host-a-old"]);
        assert_eq!(warning.hostnames, vec!["WORKSTATION ", "workstation"]);
    }

    #[test]
    fn sync_status_report_ignores_stale_duplicate_hostnames() {
        let store = storage::LocalStore::open(":memory:").unwrap();
        store.set_last_cursor("peer-a", 10).unwrap();
        store.set_last_cursor("peer-b", 20).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let tracker_peers = vec![
            SyncStatusPeerMetadata {
                peer_id: "peer-a".to_string(),
                device_id: Some("host-a-old".to_string()),
                hostname: Some("workstation".to_string()),
                rr_version: None,
                last_seen_unix: now - ACTIVE_DUPLICATE_LAST_SEEN_MAX_AGE_SEC - 1,
            },
            SyncStatusPeerMetadata {
                peer_id: "peer-b".to_string(),
                device_id: Some("host-a-new".to_string()),
                hostname: Some("workstation".to_string()),
                rr_version: None,
                last_seen_unix: now - 30,
            },
        ];

        let report =
            build_sync_status_report(&store, "local-device", None, None, None, &tracker_peers)
                .unwrap();

        assert!(report.warnings.is_empty());
    }

    #[test]
    fn sync_status_report_ignores_unknown_duplicate_hostnames() {
        let store = storage::LocalStore::open(":memory:").unwrap();
        store.set_last_cursor("peer-a", 10).unwrap();
        store.set_last_cursor("peer-b", 20).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let tracker_peers = vec![
            SyncStatusPeerMetadata {
                peer_id: "peer-a".to_string(),
                device_id: Some("host-a".to_string()),
                hostname: Some("unknown".to_string()),
                rr_version: None,
                last_seen_unix: now - 30,
            },
            SyncStatusPeerMetadata {
                peer_id: "peer-b".to_string(),
                device_id: Some("host-b".to_string()),
                hostname: Some(" UNKNOWN ".to_string()),
                rr_version: None,
                last_seen_unix: now - 40,
            },
        ];

        let report =
            build_sync_status_report(&store, "local-device", None, None, None, &tracker_peers)
                .unwrap();

        assert!(report.warnings.is_empty());
    }

    #[test]
    fn sync_status_report_warns_for_active_duplicate_device_ids() {
        let store = storage::LocalStore::open(":memory:").unwrap();
        store.set_last_cursor("peer-a", 10).unwrap();
        store.set_last_cursor("peer-b", 20).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let tracker_peers = vec![
            SyncStatusPeerMetadata {
                peer_id: "peer-a".to_string(),
                device_id: Some("same-device".to_string()),
                hostname: Some("host-a".to_string()),
                rr_version: None,
                last_seen_unix: now - 30,
            },
            SyncStatusPeerMetadata {
                peer_id: "peer-b".to_string(),
                device_id: Some(" same-device ".to_string()),
                hostname: Some("host-b".to_string()),
                rr_version: None,
                last_seen_unix: now - 40,
            },
        ];

        let report =
            build_sync_status_report(&store, "local-device", None, None, None, &tracker_peers)
                .unwrap();

        assert_eq!(report.warnings.len(), 1);
        let warning = &report.warnings[0];
        assert_eq!(warning.code, "active_duplicate_device_id");
        assert_eq!(warning.peer_ids, vec!["peer-a", "peer-b"]);
    }

    #[test]
    fn sync_status_report_warns_for_active_duplicate_local_identity_hidden_from_rows() {
        let store = storage::LocalStore::open(":memory:").unwrap();
        store.set_last_cursor("old-peer", 10).unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let tracker_peers = vec![
            SyncStatusPeerMetadata {
                peer_id: "current-peer".to_string(),
                device_id: Some("local-device".to_string()),
                hostname: Some("workstation".to_string()),
                rr_version: None,
                last_seen_unix: now - 10,
            },
            SyncStatusPeerMetadata {
                peer_id: "old-peer".to_string(),
                device_id: Some("local-device".to_string()),
                hostname: Some("workstation".to_string()),
                rr_version: None,
                last_seen_unix: now - 20,
            },
        ];

        let report = build_sync_status_report(
            &store,
            "local-device",
            Some("current-peer"),
            None,
            None,
            &tracker_peers,
        )
        .unwrap();

        assert!(report.peers.is_empty());
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "active_duplicate_hostname")
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.code == "active_duplicate_device_id")
        );
    }
}
