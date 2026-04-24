use crate::core::Entry;
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::time::Duration;
use time::OffsetDateTime;

pub const DEFAULT_DB_PATH: &str = "~/.rustory/history.db";

pub struct PullBatch {
    pub entries: Vec<Entry>,
    pub next_cursor: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertStats {
    pub inserted: usize,
    pub ignored: usize,
}

pub struct LocalStore {
    conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerBookPeer {
    pub peer_id: String,
    pub addrs: Vec<String>,
    pub user_id: Option<String>,
    pub device_id: Option<String>,
    pub last_seen_unix: i64,
}

impl LocalStore {
    pub fn open(path: &str) -> Result<Self> {
        let path = expand_home(path)?;
        ensure_parent_dir(&path)?;

        let conn = Connection::open(path).context("open sqlite db")?;
        conn.busy_timeout(Duration::from_secs(5))
            .context("set sqlite busy_timeout")?;
        init_schema(&conn).context("init schema")?;
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
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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

        Ok(PullBatch {
            entries,
            next_cursor: last_cursor,
        })
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

        Ok(PullBatch {
            entries,
            next_cursor: last_cursor,
        })
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

    pub fn upsert_peer_book(&self, peer: &PeerBookPeer) -> Result<()> {
        let addrs_json = serde_json::to_string(&peer.addrs).context("serialize peer_book addrs")?;
        self.conn
            .execute(
                r#"
INSERT INTO peer_book(peer_id, addrs_json, user_id, device_id, last_seen)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT(peer_id) DO UPDATE SET
  addrs_json = excluded.addrs_json,
  user_id = excluded.user_id,
  device_id = excluded.device_id,
  last_seen = excluded.last_seen
"#,
                params![
                    peer.peer_id,
                    addrs_json,
                    peer.user_id,
                    peer.device_id,
                    peer.last_seen_unix,
                ],
            )
            .context("upsert peer_book")?;
        Ok(())
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
SELECT peer_id, addrs_json, user_id, device_id, last_seen
FROM peer_book
WHERE (user_id = ?1 OR user_id IS NULL)
  AND last_seen >= ?2
ORDER BY last_seen DESC, peer_id ASC
LIMIT ?3
"#
        } else {
            r#"
SELECT peer_id, addrs_json, user_id, device_id, last_seen
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
}

fn row_to_peer_book_peer(row: &rusqlite::Row<'_>) -> rusqlite::Result<PeerBookPeer> {
    let peer_id: String = row.get(0)?;
    let addrs_json: String = row.get(1)?;
    let user_id: Option<String> = row.get(2)?;
    let device_id: Option<String> = row.get(3)?;
    let last_seen_unix: i64 = row.get(4)?;
    let addrs: Vec<String> = serde_json::from_str(&addrs_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(PeerBookPeer {
        peer_id,
        addrs,
        user_id,
        device_id,
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
    std::fs::create_dir_all(parent).with_context(|| format!("create dir: {}", parent.display()))?;
    Ok(())
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

CREATE TABLE IF NOT EXISTS peer_state (
  peer_id TEXT PRIMARY KEY,
  last_cursor INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS peer_push_state (
  peer_id TEXT PRIMARY KEY,
  last_pushed_seq INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS peer_book (
  peer_id TEXT PRIMARY KEY,
  addrs_json TEXT NOT NULL,
  user_id TEXT,
  device_id TEXT,
  last_seen INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_peer_book_last_seen ON peer_book(last_seen);
"#,
    )
    .context("execute schema batch")?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

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
        assert!(names.iter().any(|n| n == "peer_state"));
        assert!(names.iter().any(|n| n == "peer_push_state"));
        assert!(names.iter().any(|n| n == "peer_book"));
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
    fn peer_book_upsert_and_list_filters_by_user_and_age() {
        let store = LocalStore::open(":memory:").unwrap();

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1/p2p/peer-a".to_string()],
                user_id: Some("u1".to_string()),
                device_id: Some("d1".to_string()),
                last_seen_unix: 100,
            })
            .unwrap();

        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: "peer-b".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/2/p2p/peer-b".to_string()],
                user_id: Some("u2".to_string()),
                device_id: Some("d2".to_string()),
                last_seen_unix: 200,
            })
            .unwrap();

        let got = store.list_peer_book(Some("u1"), 0, 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].peer_id, "peer-a");

        let got = store.list_peer_book(None, 150, 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].peer_id, "peer-b");
    }
}
