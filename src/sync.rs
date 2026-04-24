use crate::storage::{LocalStore, PullBatch};
use anyhow::{Context, Result};
use std::{future::Future, pin::Pin};

/// peer로부터 pull 기반으로 cursor를 따라잡는다.
///
/// - cursor는 local의 `peer_state.last_cursor(peer_id)`를 사용한다.
/// - remote pull은 클로저로 주입한다(HTTP/P2P 등 transport에 독립적).
pub fn sync_pull_from_peer<F>(
    local: &LocalStore,
    peer_id: &str,
    limit: usize,
    mut pull: F,
) -> Result<usize>
where
    F: FnMut(i64, usize) -> Result<PullBatch>,
{
    if limit == 0 {
        return Ok(0);
    }

    let mut cursor = local.get_last_cursor(peer_id)?;
    let mut pulled_total = 0usize;
    let mut batch_limit = limit;

    loop {
        let batch = match pull(cursor, batch_limit) {
            Ok(v) => v,
            Err(err) => {
                if is_payload_too_large_error(&err) {
                    if batch_limit <= 1 {
                        return Err(err).context("pull batch too large even with limit=1");
                    }
                    batch_limit = (batch_limit / 2).max(1);
                    continue;
                }
                return Err(err);
            }
        };
        if batch.entries.is_empty() {
            break;
        }

        local.insert_entries(&batch.entries)?;
        pulled_total += batch.entries.len();

        let Some(next_cursor) = batch.next_cursor else {
            anyhow::bail!("invalid pull batch: entries is non-empty but next_cursor is None");
        };
        if next_cursor <= cursor {
            anyhow::bail!("invalid pull batch: next_cursor did not advance");
        }
        cursor = next_cursor;
        local.set_last_cursor(peer_id, cursor)?;
    }

    Ok(pulled_total)
}

pub trait Puller {
    fn pull<'a>(
        &'a mut self,
        cursor: i64,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<PullBatch>> + 'a>>;
}

pub trait Pusher {
    fn push<'a>(
        &'a mut self,
        entries: Vec<crate::core::Entry>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>>;
}

pub async fn sync_pull_from_peer_async<P>(
    local: &LocalStore,
    peer_id: &str,
    limit: usize,
    puller: &mut P,
) -> Result<usize>
where
    P: Puller,
{
    if limit == 0 {
        return Ok(0);
    }

    let mut cursor = local.get_last_cursor(peer_id)?;
    let mut pulled_total = 0usize;
    let mut batch_limit = limit;

    loop {
        let batch = match puller.pull(cursor, batch_limit).await {
            Ok(v) => v,
            Err(err) => {
                if is_payload_too_large_error(&err) {
                    if batch_limit <= 1 {
                        return Err(err).context("pull batch too large even with limit=1");
                    }
                    batch_limit = (batch_limit / 2).max(1);
                    continue;
                }
                return Err(err);
            }
        };
        if batch.entries.is_empty() {
            break;
        }

        local.insert_entries(&batch.entries)?;
        pulled_total += batch.entries.len();

        let Some(next_cursor) = batch.next_cursor else {
            anyhow::bail!("invalid pull batch: entries is non-empty but next_cursor is None");
        };
        if next_cursor <= cursor {
            anyhow::bail!("invalid pull batch: next_cursor did not advance");
        }
        cursor = next_cursor;
        local.set_last_cursor(peer_id, cursor)?;
    }

    Ok(pulled_total)
}

/// local에서 peer로 push 기반으로 cursor를 따라잡는다.
///
/// - cursor는 local의 `peer_push_state.last_pushed_seq(peer_id)`를 사용한다.
/// - push 대상은 "로컬 ingest_seq" 기준으로 배치 전송한다.
pub fn sync_push_to_peer<F>(
    local: &LocalStore,
    peer_id: &str,
    limit: usize,
    source_device_id: Option<&str>,
    mut push: F,
) -> Result<usize>
where
    F: FnMut(Vec<crate::core::Entry>) -> Result<()>,
{
    if limit == 0 {
        return Ok(0);
    }

    let mut cursor = local.get_last_pushed_seq(peer_id)?;
    let mut pushed_total = 0usize;
    let mut batch_limit = limit;

    loop {
        let batch = match source_device_id {
            Some(device_id) => {
                local.pull_since_cursor_for_device(cursor, batch_limit, device_id)?
            }
            None => local.pull_since_cursor(cursor, batch_limit)?,
        };
        if batch.entries.is_empty() {
            break;
        }

        let entries = batch.entries;
        let entries_len = entries.len();
        match push(entries) {
            Ok(()) => {}
            Err(err) => {
                if is_payload_too_large_error(&err) {
                    if batch_limit <= 1 {
                        return Err(err).context("push batch too large even with limit=1");
                    }
                    batch_limit = (batch_limit / 2).max(1);
                    continue;
                }
                return Err(err);
            }
        }
        pushed_total += entries_len;

        let Some(next_cursor) = batch.next_cursor else {
            anyhow::bail!("invalid local push batch: entries is non-empty but next_cursor is None");
        };
        if next_cursor <= cursor {
            anyhow::bail!("invalid local push batch: next_cursor did not advance");
        }
        cursor = next_cursor;
        local.set_last_pushed_seq(peer_id, cursor)?;
    }

    Ok(pushed_total)
}

pub async fn sync_push_to_peer_async<P>(
    local: &LocalStore,
    peer_id: &str,
    limit: usize,
    source_device_id: Option<&str>,
    pusher: &mut P,
) -> Result<usize>
where
    P: Pusher,
{
    if limit == 0 {
        return Ok(0);
    }

    let mut cursor = local.get_last_pushed_seq(peer_id)?;
    let mut pushed_total = 0usize;
    let mut batch_limit = limit;

    loop {
        let batch = match source_device_id {
            Some(device_id) => {
                local.pull_since_cursor_for_device(cursor, batch_limit, device_id)?
            }
            None => local.pull_since_cursor(cursor, batch_limit)?,
        };
        if batch.entries.is_empty() {
            break;
        }

        let entries = batch.entries;
        let entries_len = entries.len();
        match pusher.push(entries).await {
            Ok(()) => {}
            Err(err) => {
                if is_payload_too_large_error(&err) {
                    if batch_limit <= 1 {
                        return Err(err).context("push batch too large even with limit=1");
                    }
                    batch_limit = (batch_limit / 2).max(1);
                    continue;
                }
                return Err(err);
            }
        }
        pushed_total += entries_len;

        let Some(next_cursor) = batch.next_cursor else {
            anyhow::bail!("invalid local push batch: entries is non-empty but next_cursor is None");
        };
        if next_cursor <= cursor {
            anyhow::bail!("invalid local push batch: next_cursor did not advance");
        }
        cursor = next_cursor;
        local.set_last_pushed_seq(peer_id, cursor)?;
    }

    Ok(pushed_total)
}

pub(crate) fn is_payload_too_large_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        if let Some(e) = cause.downcast_ref::<ureq::Error>() {
            return matches!(e, ureq::Error::Status(413, _));
        }

        if let Some(libp2p_request_response::OutboundFailure::Io(ioe)) =
            cause.downcast_ref::<libp2p_request_response::OutboundFailure>()
        {
            return ioe.to_string().contains("too large");
        }

        // fallback: error text 기반(transport 레이어가 string-only로 감싸는 경우를 대비)
        let s = cause.to_string();
        s.contains("message too large")
            || s.contains("request too large")
            || s.contains("payload too large")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Entry;
    use futures::executor;
    use libp2p_request_response::OutboundFailure;
    use time::OffsetDateTime;

    struct StorePuller<'a> {
        remote: &'a LocalStore,
    }

    impl Puller for StorePuller<'_> {
        fn pull<'a>(
            &'a mut self,
            cursor: i64,
            limit: usize,
        ) -> Pin<Box<dyn Future<Output = Result<PullBatch>> + 'a>> {
            let remote = self.remote;
            Box::pin(async move { remote.pull_since_cursor(cursor, limit) })
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

    fn payload_too_large_err() -> anyhow::Error {
        let ioe = std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "request too large: 100 > 10 bytes",
        );
        anyhow::Error::new(OutboundFailure::Io(ioe))
    }

    #[test]
    fn sync_pulls_until_caught_up_and_persists_cursor() {
        let local = LocalStore::open(":memory:").unwrap();
        let remote = LocalStore::open(":memory:").unwrap();

        remote
            .insert_entries(&[entry("id-1", 1, "echo 1"), entry("id-2", 2, "echo 2")])
            .unwrap();

        let pulled = sync_pull_from_peer(&local, "peer-1", 100, |cursor, limit| {
            remote.pull_since_cursor(cursor, limit)
        })
        .unwrap();

        assert_eq!(pulled, 2);
        assert_eq!(local.list_recent(10).unwrap().len(), 2);
        assert_eq!(local.get_last_cursor("peer-1").unwrap(), 2);
    }

    #[test]
    fn sync_is_idempotent_when_run_multiple_times() {
        let local = LocalStore::open(":memory:").unwrap();
        let remote = LocalStore::open(":memory:").unwrap();

        remote
            .insert_entries(&[entry("id-1", 1, "echo 1"), entry("id-2", 2, "echo 2")])
            .unwrap();

        let a = sync_pull_from_peer(&local, "peer-1", 1, |cursor, limit| {
            remote.pull_since_cursor(cursor, limit)
        })
        .unwrap();
        assert_eq!(a, 2);
        assert_eq!(local.list_recent(10).unwrap().len(), 2);

        let b = sync_pull_from_peer(&local, "peer-1", 1, |cursor, limit| {
            remote.pull_since_cursor(cursor, limit)
        })
        .unwrap();
        assert_eq!(b, 0);
        assert_eq!(local.list_recent(10).unwrap().len(), 2);
    }

    #[test]
    fn sync_async_pulls_until_caught_up_and_persists_cursor() {
        let local = LocalStore::open(":memory:").unwrap();
        let remote = LocalStore::open(":memory:").unwrap();

        remote
            .insert_entries(&[entry("id-1", 1, "echo 1"), entry("id-2", 2, "echo 2")])
            .unwrap();

        let mut puller = StorePuller { remote: &remote };
        let pulled = executor::block_on(sync_pull_from_peer_async(
            &local,
            "peer-1",
            100,
            &mut puller,
        ))
        .unwrap();

        assert_eq!(pulled, 2);
        assert_eq!(local.list_recent(10).unwrap().len(), 2);
        assert_eq!(local.get_last_cursor("peer-1").unwrap(), 2);
    }

    #[test]
    fn sync_async_is_idempotent_when_run_multiple_times() {
        let local = LocalStore::open(":memory:").unwrap();
        let remote = LocalStore::open(":memory:").unwrap();

        remote
            .insert_entries(&[entry("id-1", 1, "echo 1"), entry("id-2", 2, "echo 2")])
            .unwrap();

        let mut puller = StorePuller { remote: &remote };
        let a = executor::block_on(sync_pull_from_peer_async(&local, "peer-1", 1, &mut puller))
            .unwrap();
        assert_eq!(a, 2);
        assert_eq!(local.list_recent(10).unwrap().len(), 2);

        let b = executor::block_on(sync_pull_from_peer_async(&local, "peer-1", 1, &mut puller))
            .unwrap();
        assert_eq!(b, 0);
        assert_eq!(local.list_recent(10).unwrap().len(), 2);
    }

    #[test]
    fn push_is_idempotent_when_run_multiple_times() {
        let local = LocalStore::open(":memory:").unwrap();
        let remote = LocalStore::open(":memory:").unwrap();

        local
            .insert_entries(&[entry("id-1", 1, "echo 1"), entry("id-2", 2, "echo 2")])
            .unwrap();

        let a = sync_push_to_peer(&local, "peer-1", 1, Some("dev1"), |entries| {
            remote.insert_entries(&entries)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(a, 2);
        assert_eq!(remote.list_recent(10).unwrap().len(), 2);

        let b = sync_push_to_peer(&local, "peer-1", 1, Some("dev1"), |entries| {
            remote.insert_entries(&entries)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(b, 0);
        assert_eq!(remote.list_recent(10).unwrap().len(), 2);
    }

    #[test]
    fn push_does_not_advance_cursor_when_push_fails() {
        let local = LocalStore::open(":memory:").unwrap();

        local
            .insert_entries(&[entry("id-1", 1, "echo 1"), entry("id-2", 2, "echo 2")])
            .unwrap();

        let err = sync_push_to_peer(&local, "peer-1", 100, Some("dev1"), |_entries| {
            anyhow::bail!("network error");
        })
        .unwrap_err();
        assert!(err.to_string().contains("network error"));
        assert_eq!(local.get_last_pushed_seq("peer-1").unwrap(), 0);
    }

    #[test]
    fn push_filters_by_device_id_and_avoids_gossip() {
        let local = LocalStore::open(":memory:").unwrap();
        let remote = LocalStore::open(":memory:").unwrap();

        let mut e1 = entry("id-1", 1, "echo local 1");
        e1.device_id = "dev-local".to_string();
        let mut e2 = entry("id-2", 2, "echo remote 2");
        e2.device_id = "dev-remote".to_string();
        let mut e3 = entry("id-3", 3, "echo local 3");
        e3.device_id = "dev-local".to_string();

        local.insert_entries(&[e1, e2, e3]).unwrap();

        let pushed = sync_push_to_peer(&local, "peer-1", 100, Some("dev-local"), |entries| {
            remote.insert_entries(&entries)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(pushed, 2);
        assert_eq!(local.get_last_pushed_seq("peer-1").unwrap(), 3);

        let got = remote.list_recent(10).unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|e| e.entry_id == "id-1"));
        assert!(got.iter().any(|e| e.entry_id == "id-3"));
        assert!(!got.iter().any(|e| e.entry_id == "id-2"));
    }

    #[test]
    fn pull_adapts_limit_when_payload_too_large() {
        let local = LocalStore::open(":memory:").unwrap();
        let remote = LocalStore::open(":memory:").unwrap();

        remote
            .insert_entries(&[
                entry("id-1", 1, "echo 1"),
                entry("id-2", 2, "echo 2"),
                entry("id-3", 3, "echo 3"),
            ])
            .unwrap();

        let mut call_limits: Vec<usize> = Vec::new();
        let pulled = sync_pull_from_peer(&local, "peer-1", 8, |cursor, limit| {
            call_limits.push(limit);
            if limit > 1 {
                return Err(payload_too_large_err());
            }
            remote.pull_since_cursor(cursor, limit)
        })
        .unwrap();

        assert_eq!(pulled, 3);
        assert_eq!(local.list_recent(10).unwrap().len(), 3);
        assert_eq!(local.get_last_cursor("peer-1").unwrap(), 3);

        assert!(call_limits.contains(&1));
        assert!(call_limits.first().copied().unwrap_or(0) > 1);
    }

    #[test]
    fn push_adapts_limit_when_payload_too_large() {
        let local = LocalStore::open(":memory:").unwrap();
        let remote = LocalStore::open(":memory:").unwrap();

        let mut e1 = entry("id-1", 1, "echo 1");
        e1.device_id = "dev-local".to_string();
        let mut e2 = entry("id-2", 2, "echo 2");
        e2.device_id = "dev-local".to_string();
        let mut e3 = entry("id-3", 3, "echo 3");
        e3.device_id = "dev-local".to_string();

        local.insert_entries(&[e1, e2, e3]).unwrap();

        let mut call_sizes: Vec<usize> = Vec::new();
        let pushed = sync_push_to_peer(&local, "peer-1", 8, Some("dev-local"), |entries| {
            call_sizes.push(entries.len());
            if entries.len() > 1 {
                return Err(payload_too_large_err());
            }
            remote.insert_entries(&entries)?;
            Ok(())
        })
        .unwrap();

        assert_eq!(pushed, 3);
        assert_eq!(remote.list_recent(10).unwrap().len(), 3);
        assert_eq!(local.get_last_pushed_seq("peer-1").unwrap(), 3);

        assert!(call_sizes.contains(&1));
        assert!(call_sizes.first().copied().unwrap_or(0) > 1);
    }
}
