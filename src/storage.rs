use crate::core::{Entry, EntryDeletion};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::collections::BTreeSet;
#[cfg(test)]
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::Duration;
use time::OffsetDateTime;

pub const DEFAULT_DB_PATH: &str = "~/.rustory/history.db";

pub struct PullBatch {
    pub entries: Vec<Entry>,
    pub next_cursor: Option<i64>,
    pub deletions: Vec<EntryDeletion>,
    pub next_delete_cursor: Option<i64>,
}

impl PullBatch {
    pub fn entries_only(entries: Vec<Entry>, next_cursor: Option<i64>) -> Self {
        Self {
            entries,
            next_cursor,
            deletions: Vec::new(),
            next_delete_cursor: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertStats {
    pub inserted: usize,
    pub ignored: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PruneStats {
    pub matched: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DedupeStats {
    pub groups: usize,
    pub matched: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct DedupeRequest<'a> {
    pub group_by: DedupeGroup,
    pub keep: DedupeKeep,
    pub source_device_id: Option<&'a str>,
    pub older_than_unix: Option<i64>,
    pub tombstone_user_id: &'a str,
    pub tombstone_device_id: &'a str,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupeKeep {
    Newest,
    Oldest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupeGroup {
    Context,
    Command,
}

impl DedupeGroup {
    fn partition_sql(self) -> &'static str {
        match self {
            Self::Context => "device_id,hostname,cwd,cmd,exit_code,CAST(ts / 86400 AS INTEGER)",
            Self::Command => "cmd",
        }
    }

    pub fn key_label(self) -> &'static str {
        match self {
            Self::Context => "device_id,hostname,cwd,cmd,exit_code,utc_day",
            Self::Command => "cmd",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteStats {
    pub matched: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeletionApplyStats {
    pub inserted: usize,
    pub ignored: usize,
    pub deleted: usize,
}

pub struct LocalStore {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePermissionInspection {
    pub db_mode: Option<u32>,
    pub parent_mode: Option<u32>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreInspection {
    pub path: PathBuf,
    pub exists: bool,
    pub entry_count: Option<usize>,
    pub latest_ingest_seq: Option<i64>,
    pub peer_book_count: Option<usize>,
    pub sync_peer_count: Option<usize>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerBookPeer {
    pub peer_id: String,
    pub addrs: Vec<String>,
    pub user_id: Option<String>,
    pub device_id: Option<String>,
    pub rr_version: Option<String>,
    pub last_seen_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRevocation {
    pub peer_id: String,
    pub device_id: Option<String>,
    pub user_id: Option<String>,
    pub revoked_at_unix: i64,
    pub ticket_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSyncStatus {
    pub peer_id: String,
    pub last_cursor: i64,
    pub last_pushed_seq: i64,
}

impl LocalStore {
    pub fn open(path: &str) -> Result<Self> {
        let path = expand_home(path)?;
        ensure_parent_dir(&path)?;
        ensure_private_db_file(&path)?;

        let conn = Connection::open(&path).context("open sqlite db")?;
        conn.busy_timeout(Duration::from_secs(5))
            .context("set sqlite busy_timeout")?;
        init_schema(&conn).context("init schema")?;
        restrict_sqlite_file_family_permissions(&path)?;
        Ok(Self { conn })
    }

    pub fn insert_entries(&self, entries: &[Entry]) -> Result<()> {
        let _ = self.insert_entries_with_stats(entries)?;
        Ok(())
    }

    pub fn insert_entries_with_stats(&self, entries: &[Entry]) -> Result<InsertStats> {
        if entries.is_empty() {
            return Ok(InsertStats {
                inserted: 0,
                ignored: 0,
            });
        }

        let tx = self.conn.unchecked_transaction().context("begin tx")?;

        let mut inserted = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    r#"
INSERT OR IGNORE INTO entries (
  entry_id,
  device_id,
  user_id,
  ts,
  cmd,
  cwd,
  exit_code,
  duration_ms,
  shell,
  hostname,
  version
)
SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
WHERE NOT EXISTS (
  SELECT 1
  FROM entry_deletions
  WHERE entry_id = ?
)
"#,
                )
                .context("prepare insert")?;

            for e in entries {
                let ts = e.ts.unix_timestamp();
                inserted += stmt
                    .execute(params![
                        e.entry_id,
                        e.device_id,
                        e.user_id,
                        ts,
                        e.cmd,
                        e.cwd,
                        e.exit_code,
                        e.duration_ms,
                        e.shell,
                        e.hostname,
                        e.version,
                        e.entry_id,
                    ])
                    .context("insert entry")?;
            }
        }

        tx.commit().context("commit tx")?;

        Ok(InsertStats {
            inserted,
            ignored: entries.len().saturating_sub(inserted),
        })
    }

    pub fn list_recent(&self, limit: usize) -> Result<Vec<Entry>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT
  entry_id,
  device_id,
  user_id,
  ts,
  cmd,
  cwd,
  exit_code,
  duration_ms,
  shell,
  hostname,
  version
FROM entries
ORDER BY ts DESC, device_id ASC, entry_id ASC
LIMIT ?
"#,
            )
            .context("prepare list_recent")?;

        let rows = stmt
            .query_map(params![limit as i64], row_to_entry)
            .context("query list_recent")?;

        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn pull_since_cursor(&self, cursor: i64, limit: usize) -> Result<PullBatch> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT
  ingest_seq,
  entry_id,
  device_id,
  user_id,
  ts,
  cmd,
  cwd,
  exit_code,
  duration_ms,
  shell,
  hostname,
  version
FROM entries
WHERE ingest_seq > ?
ORDER BY ingest_seq ASC
LIMIT ?
"#,
            )
            .context("prepare pull_since_cursor")?;

        let rows = stmt
            .query_map(params![cursor, limit as i64], |row| {
                let ingest_seq: i64 = row.get(0)?;
                let entry = row_to_entry_with_offset(row, 1)?;
                Ok((ingest_seq, entry))
            })
            .context("query pull_since_cursor")?;

        let mut entries = Vec::new();
        let mut last_cursor: Option<i64> = None;
        for item in rows {
            let (ingest_seq, entry) = item?;
            last_cursor = Some(ingest_seq);
            entries.push(entry);
        }

        Ok(PullBatch::entries_only(entries, last_cursor))
    }

    pub fn pull_since_cursor_for_device(
        &self,
        cursor: i64,
        limit: usize,
        device_id: &str,
    ) -> Result<PullBatch> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT
  ingest_seq,
  entry_id,
  device_id,
  user_id,
  ts,
  cmd,
  cwd,
  exit_code,
  duration_ms,
  shell,
  hostname,
  version
FROM entries
WHERE ingest_seq > ?
  AND device_id = ?
ORDER BY ingest_seq ASC
LIMIT ?
"#,
            )
            .context("prepare pull_since_cursor_for_device")?;

        let rows = stmt
            .query_map(params![cursor, device_id, limit as i64], |row| {
                let ingest_seq: i64 = row.get(0)?;
                let entry = row_to_entry_with_offset(row, 1)?;
                Ok((ingest_seq, entry))
            })
            .context("query pull_since_cursor_for_device")?;

        let mut entries = Vec::new();
        let mut last_cursor: Option<i64> = None;
        for item in rows {
            let (ingest_seq, entry) = item?;
            last_cursor = Some(ingest_seq);
            entries.push(entry);
        }

        Ok(PullBatch::entries_only(entries, last_cursor))
    }

    pub fn pull_sync_batch(
        &self,
        cursor: i64,
        delete_cursor: i64,
        limit: usize,
    ) -> Result<PullBatch> {
        let (deletions, next_delete_cursor) =
            self.pull_deletions_since_cursor(delete_cursor, limit)?;
        // 삭제 전파를 우선하되 entries+deletions 합계가 transport batch limit을 넘지 않게 한다.
        // 특히 limit=1에서도 tombstone 하나를 먼저 보낸 뒤 다음 batch에서 entry가 전진해야 한다.
        let entry_limit = limit.saturating_sub(deletions.len());
        let entry_batch = self.pull_since_cursor(cursor, entry_limit)?;
        Ok(PullBatch {
            entries: entry_batch.entries,
            next_cursor: entry_batch.next_cursor,
            deletions,
            next_delete_cursor,
        })
    }

    pub fn pull_sync_batch_for_device(
        &self,
        cursor: i64,
        delete_cursor: i64,
        limit: usize,
        device_id: &str,
    ) -> Result<PullBatch> {
        let (deletions, next_delete_cursor) =
            self.pull_deletions_since_cursor_for_device(delete_cursor, limit, device_id)?;
        let entry_limit = limit.saturating_sub(deletions.len());
        let entry_batch = self.pull_since_cursor_for_device(cursor, entry_limit, device_id)?;
        Ok(PullBatch {
            entries: entry_batch.entries,
            next_cursor: entry_batch.next_cursor,
            deletions,
            next_delete_cursor,
        })
    }

    fn pull_deletions_since_cursor(
        &self,
        cursor: i64,
        limit: usize,
    ) -> Result<(Vec<EntryDeletion>, Option<i64>)> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT
  delete_seq,
  entry_id,
  user_id,
  device_id,
  deleted_at
FROM entry_deletions
WHERE delete_seq > ?
ORDER BY delete_seq ASC
LIMIT ?
"#,
            )
            .context("prepare pull_deletions_since_cursor")?;

        let rows = stmt
            .query_map(params![cursor, limit as i64], |row| {
                let delete_seq: i64 = row.get(0)?;
                let deletion = row_to_entry_deletion_with_offset(row, 1)?;
                Ok((delete_seq, deletion))
            })
            .context("query pull_deletions_since_cursor")?;

        let mut deletions = Vec::new();
        let mut last_cursor = None;
        for item in rows {
            let (delete_seq, deletion) = item?;
            last_cursor = Some(delete_seq);
            deletions.push(deletion);
        }

        Ok((deletions, last_cursor))
    }

    fn pull_deletions_since_cursor_for_device(
        &self,
        cursor: i64,
        limit: usize,
        device_id: &str,
    ) -> Result<(Vec<EntryDeletion>, Option<i64>)> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT
  delete_seq,
  entry_id,
  user_id,
  device_id,
  deleted_at
FROM entry_deletions
WHERE delete_seq > ?
  AND device_id = ?
ORDER BY delete_seq ASC
LIMIT ?
"#,
            )
            .context("prepare pull_deletions_since_cursor_for_device")?;

        let rows = stmt
            .query_map(params![cursor, device_id, limit as i64], |row| {
                let delete_seq: i64 = row.get(0)?;
                let deletion = row_to_entry_deletion_with_offset(row, 1)?;
                Ok((delete_seq, deletion))
            })
            .context("query pull_deletions_since_cursor_for_device")?;

        let mut deletions = Vec::new();
        let mut last_cursor = None;
        for item in rows {
            let (delete_seq, deletion) = item?;
            last_cursor = Some(delete_seq);
            deletions.push(deletion);
        }

        Ok((deletions, last_cursor))
    }

    pub fn get_last_cursor(&self, peer_id: &str) -> Result<i64> {
        Ok(self.get_last_cursor_opt(peer_id)?.unwrap_or(0))
    }

    pub fn get_last_cursor_opt(&self, peer_id: &str) -> Result<Option<i64>> {
        match self.conn.query_row(
            "SELECT last_cursor FROM peer_state WHERE peer_id = ?",
            params![peer_id],
            |row| row.get(0),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err).context("query peer_state"),
        }
    }

    pub fn set_last_cursor(&self, peer_id: &str, cursor: i64) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO peer_state(peer_id, last_cursor)
VALUES (?, ?)
ON CONFLICT(peer_id) DO UPDATE SET last_cursor = excluded.last_cursor
"#,
                params![peer_id, cursor],
            )
            .context("upsert peer_state")?;
        Ok(())
    }

    pub fn get_last_delete_cursor(&self, peer_id: &str) -> Result<i64> {
        Ok(self.get_last_delete_cursor_opt(peer_id)?.unwrap_or(0))
    }

    pub fn get_last_delete_cursor_opt(&self, peer_id: &str) -> Result<Option<i64>> {
        match self.conn.query_row(
            "SELECT last_delete_cursor FROM peer_delete_state WHERE peer_id = ?",
            params![peer_id],
            |row| row.get(0),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err).context("query peer_delete_state"),
        }
    }

    pub fn set_last_delete_cursor(&self, peer_id: &str, cursor: i64) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO peer_delete_state(peer_id, last_delete_cursor)
VALUES (?, ?)
ON CONFLICT(peer_id) DO UPDATE SET last_delete_cursor = excluded.last_delete_cursor
"#,
                params![peer_id, cursor],
            )
            .context("upsert peer_delete_state")?;
        Ok(())
    }

    pub fn get_last_pushed_seq(&self, peer_id: &str) -> Result<i64> {
        Ok(self.get_last_pushed_seq_opt(peer_id)?.unwrap_or(0))
    }

    pub fn get_last_pushed_seq_opt(&self, peer_id: &str) -> Result<Option<i64>> {
        match self.conn.query_row(
            "SELECT last_pushed_seq FROM peer_push_state WHERE peer_id = ?",
            params![peer_id],
            |row| row.get(0),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err).context("query peer_push_state"),
        }
    }

    pub fn set_last_pushed_seq(&self, peer_id: &str, seq: i64) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO peer_push_state(peer_id, last_pushed_seq)
VALUES (?, ?)
ON CONFLICT(peer_id) DO UPDATE SET last_pushed_seq = excluded.last_pushed_seq
"#,
                params![peer_id, seq],
            )
            .context("upsert peer_push_state")?;
        Ok(())
    }

    pub fn get_last_pushed_delete_seq(&self, peer_id: &str) -> Result<i64> {
        Ok(self.get_last_pushed_delete_seq_opt(peer_id)?.unwrap_or(0))
    }

    pub fn get_last_pushed_delete_seq_opt(&self, peer_id: &str) -> Result<Option<i64>> {
        match self.conn.query_row(
            "SELECT last_pushed_delete_seq FROM peer_delete_push_state WHERE peer_id = ?",
            params![peer_id],
            |row| row.get(0),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err).context("query peer_delete_push_state"),
        }
    }

    pub fn advance_last_pushed_delete_seq(&self, peer_id: &str, seq: i64) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO peer_delete_push_state(peer_id, last_pushed_delete_seq)
VALUES (?, ?)
ON CONFLICT(peer_id) DO UPDATE SET
  last_pushed_delete_seq = MAX(
    peer_delete_push_state.last_pushed_delete_seq,
    excluded.last_pushed_delete_seq
  )
"#,
                params![peer_id, seq],
            )
            .context("advance peer_delete_push_state")?;
        Ok(())
    }

    pub fn advance_last_pushed_seq(&self, peer_id: &str, seq: i64) -> Result<()> {
        self.conn
            .execute(
                r#"
INSERT INTO peer_push_state(peer_id, last_pushed_seq)
VALUES (?, ?)
ON CONFLICT(peer_id) DO UPDATE SET
  last_pushed_seq = MAX(peer_push_state.last_pushed_seq, excluded.last_pushed_seq)
"#,
                params![peer_id, seq],
            )
            .context("advance peer_push_state")?;
        Ok(())
    }

    pub fn ensure_peer_push_state(&self, peer_id: &str) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context("begin ensure peer push state tx")?;
        tx.execute(
            "INSERT OR IGNORE INTO peer_push_state(peer_id, last_pushed_seq) VALUES (?, 0)",
            params![peer_id],
        )
        .context("initialize peer_push_state")?;
        tx.execute(
            "INSERT OR IGNORE INTO peer_delete_push_state(peer_id, last_pushed_delete_seq) VALUES (?, 0)",
            params![peer_id],
        )
        .context("initialize peer_delete_push_state")?;
        tx.commit().context("commit ensure peer push state tx")?;
        Ok(())
    }

    pub fn acknowledge_peer_pull_cursors(
        &self,
        peer_id: &str,
        cursor: i64,
        delete_cursor: i64,
    ) -> Result<()> {
        anyhow::ensure!(cursor >= 0, "peer pull cursor must not be negative");
        anyhow::ensure!(
            delete_cursor >= 0,
            "peer pull delete cursor must not be negative"
        );

        let entry_ceiling = self
            .latest_ingest_seq_high_water()?
            .max(self.get_last_pushed_seq(peer_id)?);
        let delete_ceiling = self
            .latest_delete_seq_high_water()?
            .max(self.get_last_pushed_delete_seq(peer_id)?);
        anyhow::ensure!(
            cursor <= entry_ceiling,
            "peer pull cursor {cursor} exceeds local entry ceiling {entry_ceiling}"
        );
        anyhow::ensure!(
            delete_cursor <= delete_ceiling,
            "peer pull delete cursor {delete_cursor} exceeds local deletion ceiling {delete_ceiling}"
        );

        self.ensure_peer_push_state(peer_id)?;
        self.advance_last_pushed_seq(peer_id, cursor)?;
        self.advance_last_pushed_delete_seq(peer_id, delete_cursor)?;
        Ok(())
    }

    pub fn latest_ingest_seq(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COALESCE(MAX(ingest_seq), 0) FROM entries",
                [],
                |row| row.get(0),
            )
            .context("query latest ingest_seq")
    }

    pub fn entry_count(&self) -> Result<usize> {
        query_count(&self.conn, "SELECT COUNT(*) FROM entries", "entry count")
    }

    fn latest_ingest_seq_high_water(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'entries'), 0)",
                [],
                |row| row.get(0),
            )
            .context("query ingest_seq high water mark")
    }

    fn latest_delete_seq_high_water(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'entry_deletions'), 0)",
                [],
                |row| row.get(0),
            )
            .context("query delete_seq high water mark")
    }

    pub fn list_peer_sync_status(&self) -> Result<Vec<PeerSyncStatus>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT
  ids.peer_id,
  COALESCE(ps.last_cursor, 0) AS last_cursor,
  COALESCE(pps.last_pushed_seq, 0) AS last_pushed_seq
FROM (
  SELECT peer_id FROM peer_state
  UNION
  SELECT peer_id FROM peer_push_state
  UNION
  SELECT peer_id FROM peer_delete_state
  UNION
  SELECT peer_id FROM peer_delete_push_state
  UNION
  SELECT peer_id FROM peer_book
) ids
LEFT JOIN peer_state ps ON ps.peer_id = ids.peer_id
LEFT JOIN peer_push_state pps ON pps.peer_id = ids.peer_id
ORDER BY ids.peer_id ASC
"#,
            )
            .context("prepare list_peer_sync_status")?;

        let rows = stmt
            .query_map([], |row| {
                Ok(PeerSyncStatus {
                    peer_id: row.get(0)?,
                    last_cursor: row.get(1)?,
                    last_pushed_seq: row.get(2)?,
                })
            })
            .context("query list_peer_sync_status")?;

        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    fn known_sync_peer_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT peer_id
FROM (
  SELECT peer_id FROM peer_state
  UNION
  SELECT peer_id FROM peer_push_state
  UNION
  SELECT peer_id FROM peer_delete_state
  UNION
  SELECT peer_id FROM peer_delete_push_state
  UNION
  SELECT peer_id FROM peer_book
) ids
ORDER BY peer_id ASC
"#,
            )
            .context("prepare known_sync_peer_ids")?;

        let rows = stmt
            .query_map([], |row| row.get(0))
            .context("query known_sync_peer_ids")?;

        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    #[cfg(test)]
    pub fn list_peer_book_last_seen_map(&self) -> Result<HashMap<String, i64>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT peer_id, last_seen
FROM peer_book
"#,
            )
            .context("prepare list_peer_book_last_seen_map")?;

        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .context("query list_peer_book_last_seen_map")?;

        let mut out = HashMap::new();
        for item in rows {
            let (peer_id, last_seen_unix): (String, i64) = item?;
            out.insert(peer_id, last_seen_unix);
        }

        Ok(out)
    }

    pub fn count_entries_after_seq(
        &self,
        seq: i64,
        source_device_id: Option<&str>,
    ) -> Result<usize> {
        let count: i64 = if let Some(device_id) = source_device_id {
            self.conn
                .query_row(
                    r#"
SELECT COUNT(*)
FROM entries
WHERE ingest_seq > ?
  AND device_id = ?
"#,
                    params![seq, device_id],
                    |row| row.get(0),
                )
                .context("count entries after seq for device")?
        } else {
            self.conn
                .query_row(
                    r#"
SELECT COUNT(*)
FROM entries
WHERE ingest_seq > ?
"#,
                    params![seq],
                    |row| row.get(0),
                )
                .context("count entries after seq")?
        };

        Ok(count.max(0) as usize)
    }

    pub fn count_pending_push_entries(
        &self,
        peer_id: &str,
        source_device_id: Option<&str>,
    ) -> Result<usize> {
        let last_pushed_seq = self.get_last_pushed_seq(peer_id)?;
        self.count_entries_after_seq(last_pushed_seq, source_device_id)
    }

    pub fn count_deletions_after_seq(
        &self,
        seq: i64,
        source_device_id: Option<&str>,
    ) -> Result<usize> {
        let count: i64 = if let Some(device_id) = source_device_id {
            self.conn
                .query_row(
                    r#"
SELECT COUNT(*)
FROM entry_deletions
WHERE delete_seq > ?
  AND device_id = ?
"#,
                    params![seq, device_id],
                    |row| row.get(0),
                )
                .context("count deletions after seq for device")?
        } else {
            self.conn
                .query_row(
                    r#"
SELECT COUNT(*)
FROM entry_deletions
WHERE delete_seq > ?
"#,
                    params![seq],
                    |row| row.get(0),
                )
                .context("count deletions after seq")?
        };

        Ok(count.max(0) as usize)
    }

    pub fn count_pending_push_deletions(
        &self,
        peer_id: &str,
        source_device_id: Option<&str>,
    ) -> Result<usize> {
        let last_pushed_delete_seq = self.get_last_pushed_delete_seq(peer_id)?;
        self.count_deletions_after_seq(last_pushed_delete_seq, source_device_id)
    }

    pub fn count_pending_push_items(
        &self,
        peer_id: &str,
        source_device_id: Option<&str>,
    ) -> Result<usize> {
        Ok(self.count_pending_push_entries(peer_id, source_device_id)?
            + self.count_pending_push_deletions(peer_id, source_device_id)?)
    }

    pub fn prune_entries_older_than(
        &self,
        cutoff_unix: i64,
        keep_recent: usize,
        dry_run: bool,
    ) -> Result<PruneStats> {
        let keep_recent = i64::try_from(keep_recent).context("keep_recent is too large")?;
        let tx = self
            .conn
            .unchecked_transaction()
            .context("begin prune entries tx")?;
        let pushed_floor_seq = tx
            .query_row(
                "SELECT MIN(last_pushed_seq) FROM peer_push_state",
                [],
                |row| row.get::<_, Option<i64>>(0),
            )
            .context("resolve prune pushed floor seq")?;

        let matched: i64 = tx
            .query_row(
                r#"
WITH protected AS (
  SELECT entry_id
  FROM entries
  ORDER BY ts DESC, ingest_seq DESC
  LIMIT ?2
)
SELECT COUNT(*)
FROM entries
WHERE ts < ?1
  AND entry_id NOT IN (SELECT entry_id FROM protected)
  AND (?3 IS NULL OR ingest_seq <= ?3)
"#,
                params![cutoff_unix, keep_recent, pushed_floor_seq],
                |row| row.get(0),
            )
            .context("count prune candidates")?;

        if dry_run || matched <= 0 {
            tx.commit().context("commit prune entries tx")?;
            return Ok(PruneStats {
                matched: matched.max(0) as usize,
                deleted: 0,
            });
        }

        // push cursor가 남아 있는 peer가 있으면, 가장 느린 peer가 아직 못 받은 ingest_seq는 지우지 않는다.
        let deleted = tx
            .execute(
                r#"
WITH protected AS (
  SELECT entry_id
  FROM entries
  ORDER BY ts DESC, ingest_seq DESC
  LIMIT ?2
)
DELETE FROM entries
WHERE ts < ?1
  AND entry_id NOT IN (SELECT entry_id FROM protected)
  AND (?3 IS NULL OR ingest_seq <= ?3)
"#,
                params![cutoff_unix, keep_recent, pushed_floor_seq],
            )
            .context("delete pruned entries")?;
        tx.commit().context("commit prune entries tx")?;

        Ok(PruneStats {
            matched: matched as usize,
            deleted,
        })
    }

    fn tombstone_gc_delete_floor_seq(&self) -> Result<Option<i64>> {
        let peer_ids = self.known_sync_peer_ids()?;
        if peer_ids.is_empty() {
            return Ok(None);
        }

        let mut floor = i64::MAX;
        for peer_id in peer_ids {
            let seq = self.get_last_pushed_delete_seq_opt(&peer_id)?.unwrap_or(0);
            floor = floor.min(seq);
        }

        Ok(Some(floor))
    }

    pub fn gc_tombstones_older_than(&self, cutoff_unix: i64, dry_run: bool) -> Result<PruneStats> {
        let delete_floor_seq = self.tombstone_gc_delete_floor_seq()?;

        let matched: i64 = match delete_floor_seq {
            Some(delete_floor_seq) => self
                .conn
                .query_row(
                    r#"
SELECT COUNT(*)
FROM entry_deletions
WHERE deleted_at < ?
  AND delete_seq <= ?
"#,
                    params![cutoff_unix, delete_floor_seq],
                    |row| row.get(0),
                )
                .context("count tombstone gc candidates with delete push floor")?,
            None => self
                .conn
                .query_row(
                    r#"
SELECT COUNT(*)
FROM entry_deletions
WHERE deleted_at < ?
"#,
                    params![cutoff_unix],
                    |row| row.get(0),
                )
                .context("count tombstone gc candidates")?,
        };

        if dry_run || matched <= 0 {
            return Ok(PruneStats {
                matched: matched.max(0) as usize,
                deleted: 0,
            });
        }

        // 삭제 tombstone은 오래된 peer의 재삽입을 막는 유일한 장치다.
        // 알려진 peer가 하나라도 해당 delete_seq를 못 받았으면 지우지 않는다.
        let deleted = match delete_floor_seq {
            Some(delete_floor_seq) => self
                .conn
                .execute(
                    r#"
DELETE FROM entry_deletions
WHERE deleted_at < ?
  AND delete_seq <= ?
"#,
                    params![cutoff_unix, delete_floor_seq],
                )
                .context("delete tombstones with delete push floor")?,
            None => self
                .conn
                .execute(
                    r#"
DELETE FROM entry_deletions
WHERE deleted_at < ?
"#,
                    params![cutoff_unix],
                )
                .context("delete tombstones")?,
        };

        Ok(PruneStats {
            matched: matched as usize,
            deleted,
        })
    }

    pub fn dedupe_entries(&self, request: DedupeRequest<'_>) -> Result<DedupeStats> {
        let DedupeRequest {
            group_by,
            keep,
            source_device_id,
            older_than_unix,
            tombstone_user_id,
            tombstone_device_id,
            dry_run,
        } = request;
        let pushed_floor_seq = self.prune_pushed_floor_seq()?;
        let key_columns = group_by.partition_sql();
        let order_by = match keep {
            DedupeKeep::Newest => "ts DESC, ingest_seq DESC",
            DedupeKeep::Oldest => "ts ASC, ingest_seq ASC",
        };
        let ranked_cte = format!(
            r#"
WITH ranked AS (
  SELECT
    entry_id,
    ROW_NUMBER() OVER (
      PARTITION BY {key_columns}
      ORDER BY {order_by}
    ) AS rank_in_group,
    COUNT(*) OVER (
      PARTITION BY {key_columns}
    ) AS group_count
  FROM entries
  WHERE (?1 IS NULL OR device_id = ?1)
    AND (?2 IS NULL OR ts < ?2)
    AND (?3 IS NULL OR ingest_seq <= ?3)
)
"#
        );
        let count_sql = format!(
            r#"
{ranked_cte}
SELECT
  COALESCE(SUM(CASE WHEN group_count > 1 AND rank_in_group = 1 THEN 1 ELSE 0 END), 0),
  COALESCE(SUM(CASE WHEN group_count > 1 AND rank_in_group > 1 THEN 1 ELSE 0 END), 0)
FROM ranked
"#
        );

        let (groups, matched): (i64, i64) = self
            .conn
            .query_row(
                count_sql.as_str(),
                params![source_device_id, older_than_unix, pushed_floor_seq],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .context("count dedupe candidates")?;

        let groups = groups.max(0) as usize;
        let matched = matched.max(0) as usize;
        if dry_run || matched == 0 {
            return Ok(DedupeStats {
                groups,
                matched,
                deleted: 0,
            });
        }

        let select_sql = format!(
            r#"
{ranked_cte}
SELECT entry_id
FROM ranked
WHERE group_count > 1
  AND rank_in_group > 1
"#
        );
        let mut stmt = self
            .conn
            .prepare(select_sql.as_str())
            .context("prepare duplicate entry tombstones")?;
        let rows = stmt
            .query_map(
                params![source_device_id, older_than_unix, pushed_floor_seq],
                |row| row.get::<_, String>(0),
            )
            .context("query duplicate entry tombstones")?;
        let deleted_at = OffsetDateTime::now_utc().unix_timestamp();
        let mut deletions = Vec::new();
        for row in rows {
            let entry_id = row?;
            deletions.push(EntryDeletion {
                entry_id,
                user_id: tombstone_user_id.to_string(),
                device_id: tombstone_device_id.to_string(),
                deleted_at,
            });
        }
        let deletion_stats = self.apply_entry_deletions_with_stats(&deletions)?;

        Ok(DedupeStats {
            groups,
            matched,
            deleted: deletion_stats.deleted,
        })
    }

    pub fn entry_ids_matching_cmd_regex(&self, re: &regex::Regex) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT entry_id, cmd FROM entries ORDER BY ingest_seq ASC")
            .context("prepare entry id scan by command regex")?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("query entry id scan by command regex")?;

        let mut ids = Vec::new();
        for row in rows {
            let (entry_id, cmd) = row?;
            if re.is_match(&cmd) {
                ids.push(entry_id);
            }
        }
        Ok(ids)
    }

    pub fn tombstone_entries_by_ids(
        &self,
        entry_ids: &[String],
        user_id: &str,
        device_id: &str,
        dry_run: bool,
    ) -> Result<DeleteStats> {
        let ids: Vec<String> = entry_ids
            .iter()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(ToString::to_string)
            .collect();

        if ids.is_empty() {
            return Ok(DeleteStats {
                matched: 0,
                deleted: 0,
            });
        }

        let mut matched_ids = Vec::new();
        for id in &ids {
            let matched = self
                .conn
                .query_row(
                    "SELECT 1 FROM entries WHERE entry_id = ? LIMIT 1",
                    params![id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .context("select entries selected for tombstone")?
                .is_some();
            if matched {
                matched_ids.push(id.clone());
            }
        }

        if dry_run || matched_ids.is_empty() {
            return Ok(DeleteStats {
                matched: matched_ids.len(),
                deleted: 0,
            });
        }

        let deleted_at = OffsetDateTime::now_utc().unix_timestamp();
        let deletions: Vec<EntryDeletion> = matched_ids
            .iter()
            .map(|entry_id| EntryDeletion {
                entry_id: entry_id.clone(),
                user_id: user_id.to_string(),
                device_id: device_id.to_string(),
                deleted_at,
            })
            .collect();
        let stats = self.apply_entry_deletions_with_stats(&deletions)?;

        Ok(DeleteStats {
            matched: matched_ids.len(),
            deleted: stats.deleted,
        })
    }

    pub fn apply_entry_deletions_with_stats(
        &self,
        deletions: &[EntryDeletion],
    ) -> Result<DeletionApplyStats> {
        if deletions.is_empty() {
            return Ok(DeletionApplyStats {
                inserted: 0,
                ignored: 0,
                deleted: 0,
            });
        }

        let tx = self
            .conn
            .unchecked_transaction()
            .context("begin entry deletion tx")?;
        let mut inserted = 0usize;
        let mut ignored = 0usize;
        let mut deleted = 0usize;

        for deletion in deletions {
            let entry_id = deletion.entry_id.trim();
            if entry_id.is_empty() {
                ignored += 1;
                continue;
            }

            let existing_deleted_at = tx
                .query_row(
                    "SELECT deleted_at FROM entry_deletions WHERE entry_id = ?",
                    params![entry_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .context("query existing entry deletion")?;

            if existing_deleted_at.is_some_and(|ts| ts >= deletion.deleted_at) {
                ignored += 1;
            } else {
                tx.execute(
                    r#"
INSERT INTO entry_deletions(entry_id, user_id, device_id, deleted_at)
VALUES (?, ?, ?, ?)
ON CONFLICT(entry_id) DO UPDATE SET
  user_id = excluded.user_id,
  device_id = excluded.device_id,
  deleted_at = excluded.deleted_at
WHERE excluded.deleted_at > entry_deletions.deleted_at
"#,
                    params![
                        entry_id,
                        deletion.user_id,
                        deletion.device_id,
                        deletion.deleted_at
                    ],
                )
                .context("upsert entry deletion")?;
                inserted += 1;
            }

            deleted += tx
                .execute("DELETE FROM entries WHERE entry_id = ?", params![entry_id])
                .context("delete tombstoned entry")?;
        }

        tx.commit().context("commit entry deletion tx")?;

        Ok(DeletionApplyStats {
            inserted,
            ignored,
            deleted,
        })
    }

    pub fn compact_storage(&self) -> Result<()> {
        self.conn
            .execute_batch(
                r#"
PRAGMA wal_checkpoint(TRUNCATE);
VACUUM;
PRAGMA wal_checkpoint(TRUNCATE);
"#,
            )
            .context("compact sqlite storage")
    }

    fn prune_pushed_floor_seq(&self) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT MIN(last_pushed_seq) FROM peer_push_state",
                [],
                |row| row.get::<_, Option<i64>>(0),
            )
            .context("resolve prune pushed floor seq")
    }

    pub fn upsert_peer_book(&self, peer: &PeerBookPeer) -> Result<()> {
        if self.is_peer_revoked(&peer.peer_id)? {
            return Ok(());
        }
        let addrs_json = serde_json::to_string(&peer.addrs).context("serialize peer_book addrs")?;
        self.conn
            .execute(
                r#"
INSERT INTO peer_book(peer_id, addrs_json, user_id, device_id, rr_version, last_seen)
VALUES (?, ?, ?, ?, ?, ?)
ON CONFLICT(peer_id) DO UPDATE SET
  addrs_json = CASE
    WHEN excluded.last_seen >= peer_book.last_seen THEN excluded.addrs_json
    ELSE peer_book.addrs_json
  END,
  user_id = CASE
    WHEN excluded.last_seen >= peer_book.last_seen AND excluded.user_id IS NOT NULL THEN excluded.user_id
    ELSE peer_book.user_id
  END,
  device_id = CASE
    WHEN excluded.last_seen >= peer_book.last_seen AND excluded.device_id IS NOT NULL THEN excluded.device_id
    ELSE peer_book.device_id
  END,
  rr_version = CASE
    WHEN excluded.last_seen >= peer_book.last_seen AND excluded.rr_version IS NOT NULL THEN excluded.rr_version
    ELSE peer_book.rr_version
  END,
  last_seen = MAX(peer_book.last_seen, excluded.last_seen)
"#,
                params![
                    peer.peer_id,
                    addrs_json,
                    peer.user_id,
                    peer.device_id,
                    peer.rr_version,
                    peer.last_seen_unix,
                ],
            )
            .context("upsert peer_book")?;
        Ok(())
    }

    pub fn apply_peer_revocation(&self, revocation: &PeerRevocation) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context("begin peer revocation tx")?;
        tx.execute(
            r#"
INSERT INTO peer_revocations(peer_id, device_id, user_id, revoked_at, ticket_id)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT(peer_id) DO UPDATE SET
  device_id = COALESCE(excluded.device_id, peer_revocations.device_id),
  user_id = COALESCE(excluded.user_id, peer_revocations.user_id),
  revoked_at = MAX(peer_revocations.revoked_at, excluded.revoked_at),
  ticket_id = excluded.ticket_id
"#,
            params![
                revocation.peer_id,
                revocation.device_id,
                revocation.user_id,
                revocation.revoked_at_unix,
                revocation.ticket_id,
            ],
        )
        .context("upsert peer revocation")?;
        for table in [
            "peer_book",
            "peer_state",
            "peer_push_state",
            "peer_delete_state",
            "peer_delete_push_state",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE peer_id = ?"),
                params![revocation.peer_id],
            )
            .with_context(|| format!("remove revoked peer from {table}"))?;
        }
        tx.commit().context("commit peer revocation tx")
    }

    pub fn is_peer_revoked(&self, peer_id: &str) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM peer_revocations WHERE peer_id = ?",
                params![peer_id],
                |row| row.get(0),
            )
            .context("query peer revocation")?;
        Ok(count > 0)
    }

    pub fn list_peer_book(
        &self,
        user_id: Option<&str>,
        min_last_seen_unix: i64,
        limit: usize,
    ) -> Result<Vec<PeerBookPeer>> {
        let mut out = Vec::new();

        let sql = if user_id.is_some() {
            r#"
SELECT peer_id, addrs_json, user_id, device_id, last_seen, rr_version
FROM peer_book
WHERE user_id = ?1
  AND last_seen >= ?2
ORDER BY last_seen DESC, peer_id ASC
LIMIT ?3
"#
        } else {
            r#"
SELECT peer_id, addrs_json, user_id, device_id, last_seen, rr_version
FROM peer_book
WHERE last_seen >= ?1
ORDER BY last_seen DESC, peer_id ASC
LIMIT ?2
"#
        };

        let mut stmt = self.conn.prepare(sql).context("prepare list_peer_book")?;
        let rows = if let Some(user_id) = user_id {
            stmt.query_map(
                params![user_id, min_last_seen_unix, limit as i64],
                row_to_peer_book_peer,
            )
            .context("query list_peer_book(user)")?
        } else {
            stmt.query_map(
                params![min_last_seen_unix, limit as i64],
                row_to_peer_book_peer,
            )
            .context("query list_peer_book(all)")?
        };

        for item in rows {
            out.push(item?);
        }

        Ok(out)
    }

    pub fn get_peer_book_peer(&self, peer_id: &str) -> Result<Option<PeerBookPeer>> {
        let mut stmt = self
            .conn
            .prepare(
                r#"
SELECT peer_id, addrs_json, user_id, device_id, last_seen, rr_version
FROM peer_book
WHERE peer_id = ?1
"#,
            )
            .context("prepare get_peer_book_peer")?;

        match stmt.query_row(params![peer_id], row_to_peer_book_peer) {
            Ok(peer) => Ok(Some(peer)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err).context("query get_peer_book_peer"),
        }
    }
}

pub fn inspect_existing_store(path: &str) -> Result<StoreInspection> {
    let path = expand_home(path)?;

    if path == Path::new(":memory:") {
        return Ok(StoreInspection {
            path,
            exists: false,
            entry_count: None,
            latest_ingest_seq: None,
            peer_book_count: None,
            sync_peer_count: None,
            error: None,
        });
    }

    let exists = match std::fs::metadata(&path) {
        Ok(md) => {
            if !md.is_file() {
                return Ok(StoreInspection {
                    path,
                    exists: true,
                    entry_count: None,
                    latest_ingest_seq: None,
                    peer_book_count: None,
                    sync_peer_count: None,
                    error: Some("path exists but is not a regular file".to_string()),
                });
            }
            true
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            return Ok(StoreInspection {
                path,
                exists: false,
                entry_count: None,
                latest_ingest_seq: None,
                peer_book_count: None,
                sync_peer_count: None,
                error: Some(format!("stat db path: {err}")),
            });
        }
    };

    if !exists {
        return Ok(StoreInspection {
            path,
            exists,
            entry_count: None,
            latest_ingest_seq: None,
            peer_book_count: None,
            sync_peer_count: None,
            error: None,
        });
    }

    match inspect_existing_store_file(&path) {
        Ok((entry_count, latest_ingest_seq, peer_book_count, sync_peer_count)) => {
            Ok(StoreInspection {
                path,
                exists,
                entry_count: Some(entry_count),
                latest_ingest_seq: Some(latest_ingest_seq),
                peer_book_count: Some(peer_book_count),
                sync_peer_count: Some(sync_peer_count),
                error: None,
            })
        }
        Err(err) => Ok(StoreInspection {
            path,
            exists,
            entry_count: None,
            latest_ingest_seq: None,
            peer_book_count: None,
            sync_peer_count: None,
            error: Some(format!("{err:#}")),
        }),
    }
}

pub fn inspect_store_permissions(path: &str) -> Result<StorePermissionInspection> {
    let path = expand_home(path)?;
    if path == Path::new(":memory:") {
        return Ok(StorePermissionInspection {
            db_mode: None,
            parent_mode: None,
            warning: None,
        });
    }

    let db_mode = file_mode_777(&path);
    let parent_mode = path.parent().and_then(file_mode_777);
    let warning = build_store_permission_warning(db_mode, parent_mode);
    Ok(StorePermissionInspection {
        db_mode,
        parent_mode,
        warning,
    })
}

fn build_store_permission_warning(
    db_mode: Option<u32>,
    parent_mode: Option<u32>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(mode) = db_mode
        && mode != 0o600
    {
        parts.push(format!("db mode={mode:03o}, want 600"));
    }
    if let Some(mode) = parent_mode
        && mode & 0o077 != 0
    {
        parts.push(format!(
            "parent mode={mode:03o}, want no group/other access"
        ));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

fn inspect_existing_store_file(path: &Path) -> Result<(usize, i64, usize, usize)> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open sqlite db read-only: {}", path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("set sqlite busy_timeout")?;

    let entry_count = query_count(&conn, "SELECT COUNT(*) FROM entries", "entry count")?;
    let latest_ingest_seq = conn
        .query_row(
            "SELECT COALESCE(MAX(ingest_seq), 0) FROM entries",
            [],
            |row| row.get::<_, i64>(0),
        )
        .context("query latest ingest_seq")?;
    let peer_book_count = query_count(&conn, "SELECT COUNT(*) FROM peer_book", "peer book count")?;
    let sync_peer_count = query_count(
        &conn,
        r#"
SELECT COUNT(*)
FROM (
  SELECT peer_id FROM peer_state
  UNION
  SELECT peer_id FROM peer_push_state
  UNION
  SELECT peer_id FROM peer_book
) peers
"#,
        "sync peer count",
    )?;

    Ok((
        entry_count,
        latest_ingest_seq,
        peer_book_count,
        sync_peer_count,
    ))
}

fn query_count(conn: &Connection, sql: &str, label: &str) -> Result<usize> {
    let count = conn
        .query_row(sql, [], |row| row.get::<_, i64>(0))
        .with_context(|| format!("query {label}"))?;
    Ok(count.max(0) as usize)
}

fn row_to_peer_book_peer(row: &rusqlite::Row<'_>) -> rusqlite::Result<PeerBookPeer> {
    let peer_id: String = row.get(0)?;
    let addrs_json: String = row.get(1)?;
    let user_id: Option<String> = row.get(2)?;
    let device_id: Option<String> = row.get(3)?;
    let last_seen_unix: i64 = row.get(4)?;
    let rr_version: Option<String> = row.get(5)?;
    let addrs: Vec<String> = serde_json::from_str(&addrs_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(PeerBookPeer {
        peer_id,
        addrs,
        user_id,
        device_id,
        rr_version,
        last_seen_unix,
    })
}

fn expand_home(path: &str) -> Result<PathBuf> {
    if path == ":memory:" {
        return Ok(PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var_os("HOME").context("HOME env var not set")?;
        return Ok(Path::new(&home).join(rest));
    }
    Ok(PathBuf::from(path))
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let existed = parent.exists();
    std::fs::create_dir_all(parent).with_context(|| format!("create dir: {}", parent.display()))?;
    if !existed {
        restrict_path_permissions(parent, 0o700)?;
    }
    Ok(())
}

fn ensure_private_db_file(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") {
        return Ok(());
    }

    if !path.exists() {
        create_private_file(path)?;
    }
    restrict_path_permissions(path, 0o600)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create sqlite db: {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> Result<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("create sqlite db: {}", path.display()))?;
    Ok(())
}

fn restrict_sqlite_file_family_permissions(path: &Path) -> Result<()> {
    if path == Path::new(":memory:") {
        return Ok(());
    }

    restrict_path_permissions(path, 0o600)?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = sqlite_sidecar_path(path, suffix);
        if sidecar.exists() {
            restrict_path_permissions(&sidecar, 0o600)?;
        }
    }
    Ok(())
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(suffix);
    PathBuf::from(raw)
}

#[cfg(unix)]
fn restrict_path_permissions(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let perms = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("set permissions {mode:03o}: {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_path_permissions(path: &Path, mode: u32) -> Result<()> {
    let _ = (path, mode);
    Ok(())
}

#[cfg(unix)]
fn file_mode_777(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let md = std::fs::metadata(path).ok()?;
    Some(md.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn file_mode_777(path: &Path) -> Option<u32> {
    let _ = path;
    None
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS entries (
  ingest_seq INTEGER PRIMARY KEY AUTOINCREMENT,
  entry_id TEXT NOT NULL UNIQUE,
  device_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  ts INTEGER NOT NULL,
  cmd TEXT NOT NULL,
  cwd TEXT NOT NULL,
  exit_code INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL,
  shell TEXT NOT NULL,
  hostname TEXT NOT NULL,
  version TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_entries_ts ON entries(ts);
CREATE INDEX IF NOT EXISTS idx_entries_device_id ON entries(device_id);

CREATE TABLE IF NOT EXISTS entry_deletions (
  delete_seq INTEGER PRIMARY KEY AUTOINCREMENT,
  entry_id TEXT NOT NULL UNIQUE,
  user_id TEXT NOT NULL,
  device_id TEXT NOT NULL,
  deleted_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_entry_deletions_device_seq
ON entry_deletions(device_id, delete_seq);

CREATE INDEX IF NOT EXISTS idx_entry_deletions_deleted_at_seq
ON entry_deletions(deleted_at, delete_seq);

CREATE TABLE IF NOT EXISTS peer_state (
  peer_id TEXT PRIMARY KEY,
  last_cursor INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS peer_push_state (
  peer_id TEXT PRIMARY KEY,
  last_pushed_seq INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS peer_delete_state (
  peer_id TEXT PRIMARY KEY,
  last_delete_cursor INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS peer_delete_push_state (
  peer_id TEXT PRIMARY KEY,
  last_pushed_delete_seq INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS peer_book (
  peer_id TEXT PRIMARY KEY,
  addrs_json TEXT NOT NULL,
  user_id TEXT,
  device_id TEXT,
  rr_version TEXT,
  last_seen INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_peer_book_last_seen ON peer_book(last_seen);

CREATE TABLE IF NOT EXISTS peer_revocations (
  peer_id TEXT PRIMARY KEY,
  device_id TEXT,
  user_id TEXT,
  revoked_at INTEGER NOT NULL,
  ticket_id TEXT NOT NULL
);
"#,
    )
    .context("execute schema batch")?;
    ensure_peer_book_rr_version_column(conn)?;
    Ok(())
}

fn ensure_peer_book_rr_version_column(conn: &Connection) -> Result<()> {
    if peer_book_has_rr_version_column(conn)? {
        return Ok(());
    }

    match conn.execute("ALTER TABLE peer_book ADD COLUMN rr_version TEXT", []) {
        Ok(_) => Ok(()),
        Err(_) if peer_book_has_rr_version_column(conn)? => Ok(()),
        Err(err) => Err(err).context("add peer_book rr_version column"),
    }
}

fn peer_book_has_rr_version_column(conn: &Connection) -> Result<bool> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(peer_book)")
        .context("prepare peer_book schema inspection")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .context("query peer_book schema")?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(columns.iter().any(|column| column == "rr_version"))
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    row_to_entry_with_offset(row, 0)
}

fn row_to_entry_with_offset(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<Entry> {
    let ts: i64 = row.get(offset + 3)?;
    let ts = OffsetDateTime::from_unix_timestamp(ts).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            offset + 3,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid unix timestamp: {ts}"),
            )),
        )
    })?;

    Ok(Entry {
        entry_id: row.get(offset)?,
        device_id: row.get(offset + 1)?,
        user_id: row.get(offset + 2)?,
        ts,
        cmd: row.get(offset + 4)?,
        cwd: row.get(offset + 5)?,
        exit_code: row.get(offset + 6)?,
        duration_ms: row.get(offset + 7)?,
        shell: row.get(offset + 8)?,
        hostname: row.get(offset + 9)?,
        version: row.get(offset + 10)?,
    })
}

fn row_to_entry_deletion_with_offset(
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> rusqlite::Result<EntryDeletion> {
    Ok(EntryDeletion {
        entry_id: row.get(offset)?,
        user_id: row.get(offset + 1)?,
        device_id: row.get(offset + 2)?,
        deleted_at: row.get(offset + 3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::sync::Mutex;
    use time::OffsetDateTime;

    #[cfg(unix)]
    static UMASK_LOCK: Mutex<()> = Mutex::new(());

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

    #[cfg(unix)]
    #[test]
    fn open_creates_db_private_even_with_permissive_umask() {
        let _guard = UMASK_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("private").join("history.db");

        let old_umask = unsafe { libc::umask(0) };
        let open_result = LocalStore::open(db_path.to_str().unwrap());
        unsafe {
            libc::umask(old_umask);
        }
        open_result.unwrap();

        assert_eq!(file_mode_777(&db_path), Some(0o600));
        assert_eq!(file_mode_777(db_path.parent().unwrap()), Some(0o700));
    }

    #[cfg(unix)]
    #[test]
    fn inspect_store_permissions_warns_for_permissive_existing_db() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.db");
        std::fs::write(&db_path, b"not sqlite").unwrap();
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let got = inspect_store_permissions(db_path.to_str().unwrap()).unwrap();
        assert_eq!(got.db_mode, Some(0o644));
        assert!(
            got.warning
                .as_deref()
                .unwrap()
                .contains("db mode=644, want 600")
        );
    }

    #[test]
    fn open_creates_tables() {
        let store = LocalStore::open(":memory:").unwrap();

        let mut stmt = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap();
        let mut names: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        names.sort();

        assert!(names.iter().any(|n| n == "entries"));
        assert!(names.iter().any(|n| n == "entry_deletions"));
        assert!(names.iter().any(|n| n == "peer_state"));
        assert!(names.iter().any(|n| n == "peer_delete_state"));
        assert!(names.iter().any(|n| n == "peer_push_state"));
        assert!(names.iter().any(|n| n == "peer_delete_push_state"));
        assert!(names.iter().any(|n| n == "peer_book"));
    }

    #[test]
    fn inspect_existing_store_reports_missing_without_creating() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("missing.db");

        let inspection = inspect_existing_store(db_path.to_str().unwrap()).unwrap();

        assert!(!inspection.exists);
        assert_eq!(inspection.entry_count, None);
        assert!(inspection.error.is_none());
        assert!(!db_path.exists());
    }

    #[test]
    fn inspect_existing_store_reports_counts_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.db");
        let store = LocalStore::open(db_path.to_str().unwrap()).unwrap();
        store
            .insert_entries(&[entry("id-1", 1, "echo 1"), entry("id-2", 2, "echo 2")])
            .unwrap();
        store.set_last_cursor("peer-a", 2).unwrap();
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1111/p2p/peer-a".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some("dev2".to_string()),
                rr_version: None,
                last_seen_unix: 99,
            })
            .unwrap();
        drop(store);

        let inspection = inspect_existing_store(db_path.to_str().unwrap()).unwrap();

        assert!(inspection.exists);
        assert_eq!(inspection.entry_count, Some(2));
        assert_eq!(inspection.latest_ingest_seq, Some(2));
        assert_eq!(inspection.peer_book_count, Some(1));
        assert_eq!(inspection.sync_peer_count, Some(1));
        assert!(inspection.error.is_none());
    }

    #[test]
    fn insert_dedup_and_pull_by_cursor() {
        let store = LocalStore::open(":memory:").unwrap();

        let e1 = entry("id-1", 1, "echo 1");
        let e2 = entry("id-2", 2, "echo 2");
        store.insert_entries(&[e1.clone(), e2.clone()]).unwrap();

        // duplicate entry_id must be ignored
        store.insert_entries(std::slice::from_ref(&e1)).unwrap();

        let b1 = store.pull_since_cursor(0, 1).unwrap();
        assert_eq!(b1.entries.len(), 1);
        assert_eq!(b1.entries[0].entry_id, "id-1");
        assert_eq!(b1.next_cursor, Some(1));

        let b2 = store
            .pull_since_cursor(b1.next_cursor.unwrap(), 10)
            .unwrap();
        assert_eq!(b2.entries.len(), 1);
        assert_eq!(b2.entries[0].entry_id, "id-2");
        assert_eq!(b2.next_cursor, Some(2));

        let b3 = store
            .pull_since_cursor(b2.next_cursor.unwrap(), 10)
            .unwrap();
        assert!(b3.entries.is_empty());
        assert_eq!(b3.next_cursor, None);
    }

    #[test]
    fn advance_last_pushed_seq_never_moves_cursor_backward() {
        let store = LocalStore::open(":memory:").unwrap();

        store.advance_last_pushed_seq("peer-a", 10).unwrap();
        assert_eq!(store.get_last_pushed_seq("peer-a").unwrap(), 10);

        store.advance_last_pushed_seq("peer-a", 7).unwrap();
        assert_eq!(store.get_last_pushed_seq("peer-a").unwrap(), 10);

        store.advance_last_pushed_seq("peer-a", 12).unwrap();
        assert_eq!(store.get_last_pushed_seq("peer-a").unwrap(), 12);
    }

    #[test]
    fn insert_entries_with_stats_counts_inserted_and_ignored() {
        let store = LocalStore::open(":memory:").unwrap();

        let e1 = entry("id-1", 1, "echo 1");
        let e2 = entry("id-2", 2, "echo 2");

        let stats = store
            .insert_entries_with_stats(&[e1.clone(), e2.clone()])
            .unwrap();
        assert_eq!(
            stats,
            InsertStats {
                inserted: 2,
                ignored: 0
            }
        );

        let stats = store
            .insert_entries_with_stats(std::slice::from_ref(&e1))
            .unwrap();
        assert_eq!(
            stats,
            InsertStats {
                inserted: 0,
                ignored: 1
            }
        );
    }

    #[test]
    fn pull_by_cursor_can_filter_by_device_id() {
        let store = LocalStore::open(":memory:").unwrap();

        let mut e1 = entry("id-1", 1, "echo 1");
        e1.device_id = "dev-local".to_string();

        let mut e2 = entry("id-2", 2, "echo 2");
        e2.device_id = "dev-remote".to_string();

        let mut e3 = entry("id-3", 3, "echo 3");
        e3.device_id = "dev-local".to_string();

        store
            .insert_entries(&[e1.clone(), e2.clone(), e3.clone()])
            .unwrap();

        let b1 = store
            .pull_since_cursor_for_device(0, 10, "dev-local")
            .unwrap();
        assert_eq!(b1.entries.len(), 2);
        assert_eq!(b1.entries[0].entry_id, "id-1");
        assert_eq!(b1.entries[1].entry_id, "id-3");
        assert_eq!(b1.next_cursor, Some(3));

        let b2 = store
            .pull_since_cursor_for_device(b1.next_cursor.unwrap(), 10, "dev-local")
            .unwrap();
        assert!(b2.entries.is_empty());
        assert_eq!(b2.next_cursor, None);
    }

    #[test]
    fn pull_sync_batch_applies_one_total_limit_and_prioritizes_deletions() {
        let store = LocalStore::open(":memory:").unwrap();
        store
            .insert_entries(&[
                entry("id-delete", 1, "echo delete"),
                entry("id-keep", 2, "echo keep"),
            ])
            .unwrap();
        store
            .tombstone_entries_by_ids(&["id-delete".to_string()], "user1", "dev1", false)
            .unwrap();

        let deletion_batch = store.pull_sync_batch(0, 0, 1).unwrap();
        assert!(deletion_batch.entries.is_empty());
        assert_eq!(deletion_batch.deletions.len(), 1);
        assert_eq!(deletion_batch.next_cursor, None);
        assert_eq!(deletion_batch.next_delete_cursor, Some(1));

        let entry_batch = store
            .pull_sync_batch(0, deletion_batch.next_delete_cursor.unwrap(), 1)
            .unwrap();
        assert_eq!(entry_batch.entries.len(), 1);
        assert_eq!(entry_batch.entries[0].entry_id, "id-keep");
        assert!(entry_batch.deletions.is_empty());
        assert_eq!(entry_batch.next_cursor, Some(2));
        assert_eq!(entry_batch.next_delete_cursor, None);
    }

    #[test]
    fn list_recent_orders_by_ts_desc() {
        let store = LocalStore::open(":memory:").unwrap();

        let e1 = entry("id-1", 10, "echo 1");
        let e2 = entry("id-2", 20, "echo 2");
        store.insert_entries(&[e1.clone(), e2.clone()]).unwrap();

        let got = store.list_recent(10).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].entry_id, "id-2");
        assert_eq!(got[1].entry_id, "id-1");
    }

    #[test]
    fn peer_state_roundtrip() {
        let store = LocalStore::open(":memory:").unwrap();

        assert_eq!(store.get_last_cursor("peer-a").unwrap(), 0);
        store.set_last_cursor("peer-a", 42).unwrap();
        assert_eq!(store.get_last_cursor("peer-a").unwrap(), 42);
    }

    #[test]
    fn peer_push_state_roundtrip() {
        let store = LocalStore::open(":memory:").unwrap();

        assert_eq!(store.get_last_pushed_seq("peer-a").unwrap(), 0);
        store.set_last_pushed_seq("peer-a", 7).unwrap();
        assert_eq!(store.get_last_pushed_seq("peer-a").unwrap(), 7);
    }

    #[test]
    fn latest_ingest_seq_returns_zero_for_empty_store() {
        let store = LocalStore::open(":memory:").unwrap();
        assert_eq!(store.latest_ingest_seq().unwrap(), 0);
        assert_eq!(store.entry_count().unwrap(), 0);
    }

    #[test]
    fn entry_count_is_distinct_from_gapped_ingest_cursor() {
        let store = LocalStore::open(":memory:").unwrap();
        let first = entry("id-1", 1, "echo 1");
        store.insert_entries(std::slice::from_ref(&first)).unwrap();
        store.insert_entries(std::slice::from_ref(&first)).unwrap();
        store.insert_entries(&[entry("id-2", 2, "echo 2")]).unwrap();

        assert_eq!(store.latest_ingest_seq().unwrap(), 3);
        assert_eq!(store.entry_count().unwrap(), 2);
    }

    #[test]
    fn peer_pull_ack_accepts_assigned_cursor_after_entries_are_pruned() {
        let store = LocalStore::open(":memory:").unwrap();
        store
            .insert_entries(&[entry("id-1", 10, "echo 1")])
            .unwrap();

        let pruned = store.prune_entries_older_than(11, 0, false).unwrap();
        assert_eq!(pruned.deleted, 1);
        assert_eq!(store.latest_ingest_seq().unwrap(), 0);

        store.acknowledge_peer_pull_cursors("peer-a", 1, 0).unwrap();
        assert_eq!(store.get_last_pushed_seq("peer-a").unwrap(), 1);
    }

    #[test]
    fn list_peer_sync_status_merges_pull_and_push_state() {
        let store = LocalStore::open(":memory:").unwrap();

        store.set_last_cursor("peer-a", 10).unwrap();
        store.set_last_pushed_seq("peer-a", 7).unwrap();
        store.set_last_cursor("peer-b", 3).unwrap();
        store.set_last_pushed_seq("peer-c", 9).unwrap();
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1/p2p/peer-a".to_string()],
                user_id: Some("u1".to_string()),
                device_id: Some("d1".to_string()),
                rr_version: None,
                last_seen_unix: 111,
            })
            .unwrap();
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-d".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/4/p2p/peer-d".to_string()],
                user_id: Some("u1".to_string()),
                device_id: Some("d4".to_string()),
                rr_version: None,
                last_seen_unix: 222,
            })
            .unwrap();

        let got = store.list_peer_sync_status().unwrap();

        assert_eq!(
            got,
            vec![
                PeerSyncStatus {
                    peer_id: "peer-a".to_string(),
                    last_cursor: 10,
                    last_pushed_seq: 7,
                },
                PeerSyncStatus {
                    peer_id: "peer-b".to_string(),
                    last_cursor: 3,
                    last_pushed_seq: 0,
                },
                PeerSyncStatus {
                    peer_id: "peer-c".to_string(),
                    last_cursor: 0,
                    last_pushed_seq: 9,
                },
                PeerSyncStatus {
                    peer_id: "peer-d".to_string(),
                    last_cursor: 0,
                    last_pushed_seq: 0,
                },
            ]
        );
    }

    #[test]
    fn list_peer_book_last_seen_map_returns_known_peers() {
        let store = LocalStore::open(":memory:").unwrap();

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1/p2p/peer-a".to_string()],
                user_id: Some("u1".to_string()),
                device_id: Some("d1".to_string()),
                rr_version: None,
                last_seen_unix: 100,
            })
            .unwrap();
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-b".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/2/p2p/peer-b".to_string()],
                user_id: Some("u1".to_string()),
                device_id: Some("d2".to_string()),
                rr_version: None,
                last_seen_unix: 200,
            })
            .unwrap();

        let got = store.list_peer_book_last_seen_map().unwrap();
        assert_eq!(got.get("peer-a"), Some(&100));
        assert_eq!(got.get("peer-b"), Some(&200));
        assert_eq!(got.get("peer-c"), None);
    }

    #[test]
    fn count_pending_push_entries_tracks_cursor_and_device_filter() {
        let store = LocalStore::open(":memory:").unwrap();

        let mut e1 = entry("id-1", 1, "echo 1");
        e1.device_id = "dev-local".to_string();
        let mut e2 = entry("id-2", 2, "echo 2");
        e2.device_id = "dev-remote".to_string();
        let mut e3 = entry("id-3", 3, "echo 3");
        e3.device_id = "dev-local".to_string();

        store.insert_entries(&[e1, e2, e3]).unwrap();
        store.set_last_pushed_seq("peer-a", 1).unwrap();

        assert_eq!(
            store
                .count_pending_push_entries("peer-a", Some("dev-local"))
                .unwrap(),
            1
        );
        assert_eq!(store.count_pending_push_entries("peer-a", None).unwrap(), 2);

        // ingest_seq 3까지 올리면 더 이상 pending 없음
        store.set_last_pushed_seq("peer-a", 3).unwrap();
        assert_eq!(
            store
                .count_pending_push_entries("peer-a", Some("dev-local"))
                .unwrap(),
            0
        );
    }

    #[test]
    fn prune_entries_older_than_supports_dry_run_and_apply() {
        let store = LocalStore::open(":memory:").unwrap();

        let e1 = entry("id-1", 10, "echo 1");
        let e2 = entry("id-2", 20, "echo 2");
        let e3 = entry("id-3", 30, "echo 3");
        store.insert_entries(&[e1, e2, e3]).unwrap();

        let dry = store.prune_entries_older_than(30, 0, true).unwrap();
        assert_eq!(
            dry,
            PruneStats {
                matched: 2,
                deleted: 0
            }
        );
        assert_eq!(store.list_recent(10).unwrap().len(), 3);

        let applied = store.prune_entries_older_than(30, 0, false).unwrap();
        assert_eq!(
            applied,
            PruneStats {
                matched: 2,
                deleted: 2
            }
        );
        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].entry_id, "id-3");
    }

    #[test]
    fn gc_tombstones_older_than_supports_dry_run_and_apply_without_peers() {
        let store = LocalStore::open(":memory:").unwrap();

        let e1 = entry("id-1", 10, "echo 1");
        let e2 = entry("id-2", 20, "echo 2");
        store.insert_entries(&[e1, e2]).unwrap();
        store
            .apply_entry_deletions_with_stats(&[
                EntryDeletion {
                    entry_id: "id-1".to_string(),
                    user_id: "user1".to_string(),
                    device_id: "dev1".to_string(),
                    deleted_at: 100,
                },
                EntryDeletion {
                    entry_id: "id-2".to_string(),
                    user_id: "user1".to_string(),
                    device_id: "dev1".to_string(),
                    deleted_at: 120,
                },
            ])
            .unwrap();

        let dry = store.gc_tombstones_older_than(200, true).unwrap();
        assert_eq!(
            dry,
            PruneStats {
                matched: 2,
                deleted: 0
            }
        );
        assert_eq!(store.count_deletions_after_seq(0, None).unwrap(), 2);

        let applied = store.gc_tombstones_older_than(200, false).unwrap();
        assert_eq!(
            applied,
            PruneStats {
                matched: 2,
                deleted: 2
            }
        );
        assert_eq!(store.count_deletions_after_seq(0, None).unwrap(), 0);
    }

    #[test]
    fn gc_tombstones_older_than_waits_for_known_delete_push_peers() {
        let store = LocalStore::open(":memory:").unwrap();

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1/p2p/peer-a".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some("dev-a".to_string()),
                rr_version: None,
                last_seen_unix: 100,
            })
            .unwrap();
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-b".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/2/p2p/peer-b".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some("dev-b".to_string()),
                rr_version: None,
                last_seen_unix: 100,
            })
            .unwrap();

        let e1 = entry("id-1", 10, "echo 1");
        let e2 = entry("id-2", 20, "echo 2");
        store.insert_entries(&[e1, e2]).unwrap();
        store
            .apply_entry_deletions_with_stats(&[
                EntryDeletion {
                    entry_id: "id-1".to_string(),
                    user_id: "user1".to_string(),
                    device_id: "dev1".to_string(),
                    deleted_at: 100,
                },
                EntryDeletion {
                    entry_id: "id-2".to_string(),
                    user_id: "user1".to_string(),
                    device_id: "dev1".to_string(),
                    deleted_at: 120,
                },
            ])
            .unwrap();

        store.advance_last_pushed_delete_seq("peer-a", 2).unwrap();
        assert_eq!(
            store.gc_tombstones_older_than(200, false).unwrap(),
            PruneStats {
                matched: 0,
                deleted: 0
            }
        );

        store.advance_last_pushed_delete_seq("peer-b", 1).unwrap();
        assert_eq!(
            store.gc_tombstones_older_than(200, false).unwrap(),
            PruneStats {
                matched: 1,
                deleted: 1
            }
        );
        assert_eq!(store.count_deletions_after_seq(0, None).unwrap(), 1);

        store.advance_last_pushed_delete_seq("peer-b", 2).unwrap();
        assert_eq!(
            store.gc_tombstones_older_than(200, false).unwrap(),
            PruneStats {
                matched: 1,
                deleted: 1
            }
        );
        assert_eq!(store.count_deletions_after_seq(0, None).unwrap(), 0);
    }

    #[test]
    fn dedupe_entries_by_context_keeps_newest_by_default_scope() {
        let store = LocalStore::open(":memory:").unwrap();

        let e1 = entry("id-1", 86_400 + 10, "echo same");
        let e2 = entry("id-2", 86_400 + 20, "echo same");
        let e3 = entry("id-3", 86_400 + 30, "echo same");
        let e4 = entry("id-4", 172_800 + 10, "echo same");
        let mut e5 = entry("id-5", 86_400 + 40, "echo same");
        e5.exit_code = 1;
        let mut e6 = entry("id-6", 86_400 + 50, "echo same");
        e6.device_id = "dev2".to_string();
        store.insert_entries(&[e1, e2, e3, e4, e5, e6]).unwrap();

        let dry = store
            .dedupe_entries(DedupeRequest {
                group_by: DedupeGroup::Context,
                keep: DedupeKeep::Newest,
                source_device_id: Some("dev1"),
                older_than_unix: None,
                tombstone_user_id: "user1",
                tombstone_device_id: "dev1",
                dry_run: true,
            })
            .unwrap();
        assert_eq!(
            dry,
            DedupeStats {
                groups: 1,
                matched: 2,
                deleted: 0
            }
        );
        assert_eq!(store.list_recent(10).unwrap().len(), 6);

        let applied = store
            .dedupe_entries(DedupeRequest {
                group_by: DedupeGroup::Context,
                keep: DedupeKeep::Newest,
                source_device_id: Some("dev1"),
                older_than_unix: None,
                tombstone_user_id: "user1",
                tombstone_device_id: "dev1",
                dry_run: false,
            })
            .unwrap();
        assert_eq!(
            applied,
            DedupeStats {
                groups: 1,
                matched: 2,
                deleted: 2
            }
        );

        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 4);
        assert!(remaining.iter().any(|e| e.entry_id == "id-3"));
        assert!(remaining.iter().any(|e| e.entry_id == "id-4"));
        assert!(remaining.iter().any(|e| e.entry_id == "id-5"));
        assert!(remaining.iter().any(|e| e.entry_id == "id-6"));
    }

    #[test]
    fn dedupe_entries_by_context_can_keep_oldest() {
        let store = LocalStore::open(":memory:").unwrap();

        store
            .insert_entries(&[
                entry("id-1", 86_400 + 10, "echo same"),
                entry("id-2", 86_400 + 20, "echo same"),
                entry("id-3", 86_400 + 30, "echo same"),
            ])
            .unwrap();

        let applied = store
            .dedupe_entries(DedupeRequest {
                group_by: DedupeGroup::Context,
                keep: DedupeKeep::Oldest,
                source_device_id: Some("dev1"),
                older_than_unix: None,
                tombstone_user_id: "user1",
                tombstone_device_id: "dev1",
                dry_run: false,
            })
            .unwrap();
        assert_eq!(
            applied,
            DedupeStats {
                groups: 1,
                matched: 2,
                deleted: 2
            }
        );

        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].entry_id, "id-1");
    }

    #[test]
    fn dedupe_entries_by_context_respects_push_floor() {
        let store = LocalStore::open(":memory:").unwrap();

        store
            .insert_entries(&[
                entry("id-1", 86_400 + 10, "echo same"),
                entry("id-2", 86_400 + 20, "echo same"),
                entry("id-3", 86_400 + 30, "echo same"),
            ])
            .unwrap();

        store.set_last_pushed_seq("peer-slow", 1).unwrap();
        let blocked = store
            .dedupe_entries(DedupeRequest {
                group_by: DedupeGroup::Context,
                keep: DedupeKeep::Newest,
                source_device_id: Some("dev1"),
                older_than_unix: None,
                tombstone_user_id: "user1",
                tombstone_device_id: "dev1",
                dry_run: false,
            })
            .unwrap();
        assert_eq!(
            blocked,
            DedupeStats {
                groups: 0,
                matched: 0,
                deleted: 0
            }
        );
        assert_eq!(store.list_recent(10).unwrap().len(), 3);

        store.set_last_pushed_seq("peer-slow", 3).unwrap();
        let applied = store
            .dedupe_entries(DedupeRequest {
                group_by: DedupeGroup::Context,
                keep: DedupeKeep::Newest,
                source_device_id: Some("dev1"),
                older_than_unix: None,
                tombstone_user_id: "user1",
                tombstone_device_id: "dev1",
                dry_run: false,
            })
            .unwrap();
        assert_eq!(
            applied,
            DedupeStats {
                groups: 1,
                matched: 2,
                deleted: 2
            }
        );
        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].entry_id, "id-3");
    }

    #[test]
    fn dedupe_entries_by_command_crosses_context_and_honors_device_scope() {
        let store = LocalStore::open(":memory:").unwrap();

        let mut e1 = entry("id-1", 86_400 + 10, "echo same");
        e1.cwd = "/work/one".to_string();
        e1.hostname = "host-one".to_string();

        let mut e2 = entry("id-2", 172_800 + 20, "echo same");
        e2.cwd = "/work/two".to_string();
        e2.hostname = "host-two".to_string();
        e2.exit_code = 1;

        let e3 = entry("id-3", 259_200 + 30, "echo same ");

        let mut e4 = entry("id-4", 259_200 + 40, "echo same");
        e4.device_id = "dev2".to_string();

        store
            .insert_entries(&[e1, e2, e3, e4])
            .expect("insert command dedupe fixtures");

        let local_scope = store
            .dedupe_entries(DedupeRequest {
                group_by: DedupeGroup::Command,
                keep: DedupeKeep::Newest,
                source_device_id: Some("dev1"),
                older_than_unix: None,
                tombstone_user_id: "user1",
                tombstone_device_id: "dev1",
                dry_run: true,
            })
            .unwrap();
        assert_eq!(
            local_scope,
            DedupeStats {
                groups: 1,
                matched: 1,
                deleted: 0
            }
        );

        let all_devices = store
            .dedupe_entries(DedupeRequest {
                group_by: DedupeGroup::Command,
                keep: DedupeKeep::Newest,
                source_device_id: None,
                older_than_unix: None,
                tombstone_user_id: "user1",
                tombstone_device_id: "dev1",
                dry_run: false,
            })
            .unwrap();
        assert_eq!(
            all_devices,
            DedupeStats {
                groups: 1,
                matched: 2,
                deleted: 2
            }
        );

        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().any(|entry| entry.entry_id == "id-3"));
        assert!(remaining.iter().any(|entry| entry.entry_id == "id-4"));
    }

    #[test]
    fn tombstone_entries_by_ids_supports_dry_run_and_apply() {
        let store = LocalStore::open(":memory:").unwrap();

        store
            .insert_entries(&[
                entry("id-1", 10, "echo 1"),
                entry("id-2", 20, "echo 2"),
                entry("id-3", 30, "echo 3"),
            ])
            .unwrap();

        let dry = store
            .tombstone_entries_by_ids(
                &[
                    "id-1".to_string(),
                    "id-1".to_string(),
                    "missing".to_string(),
                ],
                "user1",
                "dev1",
                true,
            )
            .unwrap();
        assert_eq!(
            dry,
            DeleteStats {
                matched: 1,
                deleted: 0
            }
        );
        assert_eq!(store.list_recent(10).unwrap().len(), 3);
        assert_eq!(store.count_deletions_after_seq(0, None).unwrap(), 0);

        let applied = store
            .tombstone_entries_by_ids(
                &["id-1".to_string(), "id-3".to_string()],
                "user1",
                "dev1",
                false,
            )
            .unwrap();
        assert_eq!(
            applied,
            DeleteStats {
                matched: 2,
                deleted: 2
            }
        );

        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].entry_id, "id-2");
        assert_eq!(store.count_deletions_after_seq(0, Some("dev1")).unwrap(), 2);
    }

    #[test]
    fn tombstoned_entries_do_not_resurrect_on_insert() {
        let store = LocalStore::open(":memory:").unwrap();
        let e1 = entry("id-1", 10, "echo 1");
        let e2 = entry("id-2", 20, "echo 2");
        store.insert_entries(&[e1.clone(), e2.clone()]).unwrap();

        store
            .tombstone_entries_by_ids(&["id-1".to_string()], "user1", "dev1", false)
            .unwrap();
        assert_eq!(store.list_recent(10).unwrap().len(), 1);

        let stats = store.insert_entries_with_stats(&[e1]).unwrap();
        assert_eq!(
            stats,
            InsertStats {
                inserted: 0,
                ignored: 1
            }
        );
        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].entry_id, "id-2");
    }

    #[test]
    fn count_pending_push_items_tracks_entries_and_deletions() {
        let store = LocalStore::open(":memory:").unwrap();

        let mut e1 = entry("id-1", 1, "echo 1");
        e1.device_id = "dev-local".to_string();
        let mut e2 = entry("id-2", 2, "echo 2");
        e2.device_id = "dev-local".to_string();

        store.insert_entries(&[e1, e2]).unwrap();
        store
            .tombstone_entries_by_ids(&["id-1".to_string()], "user1", "dev-local", false)
            .unwrap();

        assert_eq!(
            store
                .count_pending_push_items("peer-a", Some("dev-local"))
                .unwrap(),
            2
        );

        store.set_last_pushed_seq("peer-a", 2).unwrap();
        assert_eq!(
            store
                .count_pending_push_items("peer-a", Some("dev-local"))
                .unwrap(),
            1
        );

        store.advance_last_pushed_delete_seq("peer-a", 1).unwrap();
        assert_eq!(
            store
                .count_pending_push_items("peer-a", Some("dev-local"))
                .unwrap(),
            0
        );
    }

    #[test]
    fn entry_ids_matching_cmd_regex_returns_matching_commands() {
        let store = LocalStore::open(":memory:").unwrap();

        store
            .insert_entries(&[
                entry("id-1", 10, "echo safe"),
                entry("id-2", 20, "curl -H 'Authorization: Bearer secret'"),
                entry("id-3", 30, "export TOKEN=value"),
            ])
            .unwrap();

        let re = regex::Regex::new("(?i)(authorization:|token=)").unwrap();
        let ids = store.entry_ids_matching_cmd_regex(&re).unwrap();

        assert_eq!(ids, vec!["id-2".to_string(), "id-3".to_string()]);
    }

    #[test]
    fn prune_entries_older_than_respects_keep_recent() {
        let store = LocalStore::open(":memory:").unwrap();

        let e1 = entry("id-1", 10, "echo 1");
        let e2 = entry("id-2", 20, "echo 2");
        let e3 = entry("id-3", 30, "echo 3");
        store.insert_entries(&[e1, e2, e3]).unwrap();

        let stats = store.prune_entries_older_than(40, 2, false).unwrap();
        assert_eq!(
            stats,
            PruneStats {
                matched: 1,
                deleted: 1
            }
        );

        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].entry_id, "id-3");
        assert_eq!(remaining[1].entry_id, "id-2");

        let stats_keep_all = store.prune_entries_older_than(40, 10, false).unwrap();
        assert_eq!(
            stats_keep_all,
            PruneStats {
                matched: 0,
                deleted: 0
            }
        );
    }

    #[test]
    fn prune_keep_recent_uses_command_time_when_old_history_is_imported_later() {
        let store = LocalStore::open(":memory:").unwrap();

        let current = entry("current", 30, "echo current");
        let imported_old = entry("imported-old", 10, "echo imported old");
        store.insert_entries(&[current, imported_old]).unwrap();

        let stats = store.prune_entries_older_than(40, 1, false).unwrap();
        assert_eq!(
            stats,
            PruneStats {
                matched: 1,
                deleted: 1
            }
        );

        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].entry_id, "current");
    }

    #[test]
    fn prune_entries_older_than_keeps_entries_needed_by_slowest_push_peer() {
        let store = LocalStore::open(":memory:").unwrap();

        let e1 = entry("id-1", 10, "echo 1");
        let e2 = entry("id-2", 20, "echo 2");
        let e3 = entry("id-3", 30, "echo 3");
        store.insert_entries(&[e1, e2, e3]).unwrap();

        store.set_last_pushed_seq("peer-fast", 3).unwrap();
        store.set_last_pushed_seq("peer-slow", 1).unwrap();

        let stats = store.prune_entries_older_than(100, 0, false).unwrap();
        assert_eq!(
            stats,
            PruneStats {
                matched: 1,
                deleted: 1
            }
        );

        let remaining = store.list_recent(10).unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().any(|e| e.entry_id == "id-2"));
        assert!(remaining.iter().any(|e| e.entry_id == "id-3"));
        assert_eq!(
            store.count_pending_push_entries("peer-slow", None).unwrap(),
            2
        );
    }

    #[test]
    fn peer_book_upsert_and_list_filters_by_user_and_age() {
        let store = LocalStore::open(":memory:").unwrap();

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1/p2p/peer-a".to_string()],
                user_id: Some("u1".to_string()),
                device_id: Some("d1".to_string()),
                rr_version: None,
                last_seen_unix: 100,
            })
            .unwrap();

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-b".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/2/p2p/peer-b".to_string()],
                user_id: Some("u2".to_string()),
                device_id: Some("d2".to_string()),
                rr_version: None,
                last_seen_unix: 200,
            })
            .unwrap();

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-c".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/3/p2p/peer-c".to_string()],
                user_id: None,
                device_id: Some("d3".to_string()),
                rr_version: None,
                last_seen_unix: 300,
            })
            .unwrap();

        let got = store.list_peer_book(Some("u1"), 0, 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].peer_id, "peer-a");

        let got = store.list_peer_book(None, 150, 10).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].peer_id, "peer-c");
        assert_eq!(got[1].peer_id, "peer-b");
    }

    #[test]
    fn peer_revocation_removes_authorization_and_sync_floor_state() {
        let store = LocalStore::open(":memory:").unwrap();
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec![],
                user_id: Some("u1".to_string()),
                device_id: Some("node0".to_string()),
                rr_version: Some("1.0.53".to_string()),
                last_seen_unix: 100,
            })
            .unwrap();
        store.set_last_cursor("peer-a", 10).unwrap();
        store.set_last_pushed_seq("peer-a", 11).unwrap();

        store
            .apply_peer_revocation(&PeerRevocation {
                peer_id: "peer-a".to_string(),
                device_id: Some("node0".to_string()),
                user_id: Some("u1".to_string()),
                revoked_at_unix: 200,
                ticket_id: "ticket-1".to_string(),
            })
            .unwrap();

        assert!(store.is_peer_revoked("peer-a").unwrap());
        assert!(store.get_peer_book_peer("peer-a").unwrap().is_none());
        assert!(store.get_last_cursor_opt("peer-a").unwrap().is_none());
        assert!(store.get_last_pushed_seq_opt("peer-a").unwrap().is_none());
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["stale".to_string()],
                user_id: Some("u1".to_string()),
                device_id: Some("node0".to_string()),
                rr_version: None,
                last_seen_unix: 300,
            })
            .unwrap();
        assert!(store.get_peer_book_peer("peer-a").unwrap().is_none());
    }

    #[test]
    fn peer_book_upsert_keeps_latest_device_metadata() {
        let store = LocalStore::open(":memory:").unwrap();

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/198.51.100.1/tcp/1/p2p/peer-a".to_string()],
                user_id: Some("u1".to_string()),
                device_id: Some("node1".to_string()),
                rr_version: Some("1.0.44".to_string()),
                last_seen_unix: 200,
            })
            .unwrap();

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/198.51.100.2/tcp/2/p2p/peer-a".to_string()],
                user_id: None,
                device_id: Some("stale-name".to_string()),
                rr_version: Some("1.0.43".to_string()),
                last_seen_unix: 100,
            })
            .unwrap();

        let got = store.get_peer_book_peer("peer-a").unwrap().unwrap();
        assert_eq!(got.addrs, vec!["/ip4/198.51.100.1/tcp/1/p2p/peer-a"]);
        assert_eq!(got.user_id.as_deref(), Some("u1"));
        assert_eq!(got.device_id.as_deref(), Some("node1"));
        assert_eq!(got.rr_version.as_deref(), Some("1.0.44"));
        assert_eq!(got.last_seen_unix, 200);

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/198.51.100.3/tcp/3/p2p/peer-a".to_string()],
                user_id: Some("u1".to_string()),
                device_id: Some("node1-x86_64".to_string()),
                rr_version: Some("1.0.45".to_string()),
                last_seen_unix: 300,
            })
            .unwrap();

        let got = store.get_peer_book_peer("peer-a").unwrap().unwrap();
        assert_eq!(got.addrs, vec!["/ip4/198.51.100.3/tcp/3/p2p/peer-a"]);
        assert_eq!(got.user_id.as_deref(), Some("u1"));
        assert_eq!(got.device_id.as_deref(), Some("node1-x86_64"));
        assert_eq!(got.rr_version.as_deref(), Some("1.0.45"));
        assert_eq!(got.last_seen_unix, 300);
    }

    #[test]
    fn peer_book_open_migrates_existing_database_for_rr_version() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"
CREATE TABLE peer_book (
  peer_id TEXT PRIMARY KEY,
  addrs_json TEXT NOT NULL,
  user_id TEXT,
  device_id TEXT,
  last_seen INTEGER NOT NULL
);
INSERT INTO peer_book(peer_id, addrs_json, user_id, device_id, last_seen)
VALUES ('peer-a', '[]', 'u1', 'node1', 100);
"#,
            )
            .unwrap();
        }

        let store = LocalStore::open(db_path.to_str().unwrap()).unwrap();
        let existing = store.get_peer_book_peer("peer-a").unwrap().unwrap();
        assert_eq!(existing.rr_version, None);

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: Vec::new(),
                user_id: Some("u1".to_string()),
                device_id: Some("node1".to_string()),
                rr_version: Some("1.0.45".to_string()),
                last_seen_unix: 200,
            })
            .unwrap();
        let migrated = store.get_peer_book_peer("peer-a").unwrap().unwrap();
        assert_eq!(migrated.rr_version.as_deref(), Some("1.0.45"));

        drop(store);
        let reopened = LocalStore::open(db_path.to_str().unwrap()).unwrap();
        assert_eq!(
            reopened
                .get_peer_book_peer("peer-a")
                .unwrap()
                .unwrap()
                .rr_version
                .as_deref(),
            Some("1.0.45")
        );
    }
}
