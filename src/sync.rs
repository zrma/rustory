use crate::storage::{LocalStore, PullBatch};
use anyhow::Result;
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

    loop {
        let batch = pull(cursor, limit)?;
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

    loop {
        let batch = puller.pull(cursor, limit).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Entry;
    use futures::executor;
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
}
