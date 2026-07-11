use super::{
    AdapterRecord, HistoryImportAdapter, ImportStats, PathImportRequest, insert_adapter_records,
};
use crate::storage::LocalStore;
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, params, types::ValueRef};
use std::collections::BTreeSet;
use std::time::Duration as StdDuration;
use time::OffsetDateTime;

pub(super) static ADAPTER: AtuinAdapter = AtuinAdapter;

pub(super) struct AtuinAdapter;

pub(super) fn default_history_path() -> String {
    default_history_path_from_xdg(std::env::var("XDG_DATA_HOME").ok().as_deref())
}

pub(super) fn default_history_path_from_xdg(xdg_data_home: Option<&str>) -> String {
    match xdg_data_home.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => std::path::Path::new(path)
            .join("atuin/history.db")
            .to_string_lossy()
            .into_owned(),
        None => "~/.local/share/atuin/history.db".to_string(),
    }
}

impl HistoryImportAdapter for AtuinAdapter {
    fn import(&self, store: &LocalStore, req: PathImportRequest<'_>) -> Result<ImportStats> {
        let conn = Connection::open_with_flags(
            req.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("open atuin sqlite read-only: {}", req.path.display()))?;
        conn.busy_timeout(StdDuration::from_secs(5))
            .context("set atuin sqlite busy_timeout")?;

        let schema = AtuinSchema::inspect(&conn)?;
        let records = read_atuin_records(&conn, &schema, req.limit)?
            .into_iter()
            .map(|record| record.into_adapter_record(req.user_id))
            .collect::<Result<Vec<_>>>()?;

        insert_adapter_records(store, req, "atuin", records)
    }
}

struct AtuinSchema {
    has_deleted_at: bool,
}

impl AtuinSchema {
    fn inspect(conn: &Connection) -> Result<Self> {
        let mut stmt = conn
            .prepare("PRAGMA table_info(history)")
            .context("inspect atuin history schema")?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .context("query atuin history columns")?
            .collect::<rusqlite::Result<BTreeSet<_>>>()?;

        const REQUIRED: [&str; 8] = [
            "id",
            "timestamp",
            "duration",
            "exit",
            "command",
            "cwd",
            "session",
            "hostname",
        ];
        let missing = REQUIRED
            .into_iter()
            .filter(|column| !columns.contains(*column))
            .collect::<Vec<_>>();
        anyhow::ensure!(
            missing.is_empty(),
            "unsupported atuin history schema: missing columns: {}",
            missing.join(", ")
        );

        Ok(Self {
            has_deleted_at: columns.contains("deleted_at"),
        })
    }
}

struct AtuinRecord {
    source_id: Option<String>,
    timestamp_ns: i64,
    duration_ns: i64,
    exit_code: i64,
    command: String,
    cwd: String,
    session: String,
    hostname: String,
}

impl AtuinRecord {
    fn into_adapter_record(self, user_id: &str) -> Result<AdapterRecord> {
        let ts =
            OffsetDateTime::from_unix_timestamp_nanos(self.timestamp_ns as i128).map_err(|_| {
                anyhow::anyhow!("invalid atuin unix timestamp nanos: {}", self.timestamp_ns)
            })?;
        let exit_code = i32::try_from(self.exit_code)
            .with_context(|| format!("atuin exit code out of range: {}", self.exit_code))?;
        let fallback_source_key = format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}",
            self.timestamp_ns,
            self.duration_ns,
            self.exit_code,
            self.command,
            self.cwd,
            self.session,
            self.hostname
        );

        Ok(AdapterRecord {
            entry_id: atuin_import_entry_id(
                user_id,
                self.source_id.as_deref(),
                &fallback_source_key,
            ),
            ts,
            duration_ms: self.duration_ns.max(0) / 1_000_000,
            cmd: self.command,
            cwd: self.cwd,
            exit_code,
            hostname: self.hostname,
        })
    }
}

fn read_atuin_records(
    conn: &Connection,
    schema: &AtuinSchema,
    limit: Option<usize>,
) -> Result<Vec<AtuinRecord>> {
    let predicate = if schema.has_deleted_at {
        "WHERE deleted_at IS NULL"
    } else {
        ""
    };
    let sql = match limit {
        Some(_) => format!(
            r#"
SELECT source_rowid, id, timestamp, duration, exit, command, cwd, session, hostname
FROM (
  SELECT
    rowid AS source_rowid,
    id,
    timestamp,
    duration,
    exit,
    command,
    cwd,
    session,
    hostname
  FROM history
  {predicate}
  ORDER BY timestamp DESC, source_rowid DESC
  LIMIT ?1
) selected
ORDER BY timestamp ASC, source_rowid ASC
"#
        ),
        None => format!(
            r#"
SELECT rowid AS source_rowid, id, timestamp, duration, exit, command, cwd, session, hostname
FROM history
{predicate}
ORDER BY timestamp ASC, source_rowid ASC
"#
        ),
    };

    let mut stmt = conn.prepare(&sql).context("prepare atuin import query")?;
    let mut out = Vec::new();
    if let Some(limit) = limit {
        let limit = i64::try_from(limit).context("atuin import limit exceeds SQLite range")?;
        let rows = stmt
            .query_map(params![limit], row_to_atuin_record)
            .context("query limited atuin records")?;
        for row in rows {
            out.push(row?);
        }
    } else {
        let rows = stmt
            .query_map([], row_to_atuin_record)
            .context("query atuin records")?;
        for row in rows {
            out.push(row?);
        }
    }
    Ok(out)
}

fn row_to_atuin_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<AtuinRecord> {
    Ok(AtuinRecord {
        source_id: row_lossy_opt_string(row, 1)?,
        timestamp_ns: row.get(2)?,
        duration_ns: row.get(3)?,
        exit_code: row.get(4)?,
        command: row_lossy_string(row, 5)?,
        cwd: row_lossy_string(row, 6)?,
        session: row_lossy_string(row, 7)?,
        hostname: row_lossy_string(row, 8)?,
    })
}

fn row_lossy_string(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<String> {
    Ok(sqlite_value_ref_to_lossy_string(row.get_ref(index)?))
}

fn row_lossy_opt_string(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<String>> {
    let value = row.get_ref(index)?;
    Ok(match value {
        ValueRef::Null => None,
        _ => Some(sqlite_value_ref_to_lossy_string(value)),
    })
}

fn sqlite_value_ref_to_lossy_string(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => String::new(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => value.to_string(),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

fn atuin_import_entry_id(
    user_id: &str,
    source_id: Option<&str>,
    fallback_source_key: &str,
) -> crate::core::EntryId {
    let source_key = match source_id.and_then(|id| {
        let trimmed = id.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }) {
        Some(id) => format!("id\0{id}"),
        None => format!("fallback\0{fallback_source_key}"),
    };
    let name = format!("rustory:atuin-import\0{user_id}\0{source_key}");
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, name.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_import::{HistoryShell, import_path_into_store};
    use rusqlite::params;

    const SECOND: i64 = 1_000_000_000;

    #[test]
    fn atuin_import_current_schema() {
        let db = atuin_db(true, true);
        insert_row(
            db.path(),
            "atuin-1",
            1_700_000_000 * SECOND,
            2_500_000_000,
            7,
            "echo current",
            "/work",
            "session-a",
            "source-host",
            None,
        );
        let store = LocalStore::open(":memory:").unwrap();

        let stats = run_import(&store, db.path(), None).unwrap();
        assert_eq!(stats.received, 1);
        assert_eq!(stats.inserted, 1);
        let entries = store.list_recent(10).unwrap();
        assert_eq!(entries[0].cmd, "echo current");
        assert_eq!(entries[0].cwd, "/work");
        assert_eq!(entries[0].exit_code, 7);
        assert_eq!(entries[0].duration_ms, 2500);
        assert_eq!(entries[0].hostname, "source-host");
        assert_eq!(entries[0].device_id, "rustory-device");
        assert_eq!(entries[0].shell, "atuin");
    }

    #[test]
    fn atuin_default_path_respects_xdg_data_home() {
        assert_eq!(
            default_history_path_from_xdg(Some("/data/user")),
            "/data/user/atuin/history.db"
        );
        assert_eq!(
            default_history_path_from_xdg(Some("  ")),
            "~/.local/share/atuin/history.db"
        );
    }

    #[test]
    fn atuin_import_schema_variants() {
        let db = atuin_db(false, false);
        insert_row(
            db.path(),
            "atuin-old",
            1_700_000_001 * SECOND,
            0,
            0,
            "echo old-schema",
            "/old",
            "session-old",
            "old-host",
            None,
        );
        let store = LocalStore::open(":memory:").unwrap();
        let stats = run_import(&store, db.path(), None).unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(store.list_recent(10).unwrap()[0].cmd, "echo old-schema");
    }

    #[test]
    fn atuin_import_idempotency_and_deleted_rows() {
        let db = atuin_db(true, false);
        for (id, second, command, deleted_at) in [
            ("atuin-1", 1, "echo one", None),
            ("", 2, "echo two", None),
            ("atuin-3", 3, "echo deleted", Some(4 * SECOND)),
            ("atuin-4", 4, "echo four", None),
        ] {
            insert_row(
                db.path(),
                id,
                (1_700_000_000 + second) * SECOND,
                SECOND,
                0,
                command,
                "/work",
                "session",
                "host",
                deleted_at,
            );
        }
        let store = LocalStore::open(":memory:").unwrap();

        let first = run_import(&store, db.path(), Some(2)).unwrap();
        assert_eq!(first.received, 2);
        assert_eq!(first.inserted, 2);
        let second = run_import(&store, db.path(), Some(2)).unwrap();
        assert_eq!(second.inserted, 0);
        assert_eq!(second.ignored, 2);
        let commands = store
            .list_recent(10)
            .unwrap()
            .into_iter()
            .map(|entry| entry.cmd)
            .collect::<Vec<_>>();
        assert_eq!(commands, vec!["echo four", "echo two"]);
    }

    #[test]
    fn atuin_import_invalid_utf8() {
        let db = atuin_db(true, false);
        let conn = Connection::open(db.path()).unwrap();
        conn.execute(
            r#"
INSERT INTO history(id, timestamp, duration, exit, command, cwd, session, hostname, deleted_at)
VALUES (?1, ?2, 0, 0, ?3, '/work', 'session', 'host', NULL)
"#,
            params![
                "invalid-utf8",
                1_700_000_000 * SECOND,
                vec![b'e', b'c', b'h', b'o', b' ', 0xff]
            ],
        )
        .unwrap();
        let store = LocalStore::open(":memory:").unwrap();
        let stats = run_import(&store, db.path(), None).unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(store.list_recent(10).unwrap()[0].cmd, "echo �");
    }

    #[test]
    fn atuin_import_rejects_missing_required_columns() {
        let file = tempfile::NamedTempFile::new().unwrap();
        Connection::open(file.path())
            .unwrap()
            .execute_batch("CREATE TABLE history (id TEXT PRIMARY KEY, command TEXT NOT NULL);")
            .unwrap();
        let store = LocalStore::open(":memory:").unwrap();
        let error = run_import(&store, file.path(), None).unwrap_err();
        assert!(format!("{error:#}").contains("missing columns"));
    }

    #[test]
    fn atuin_import_reads_committed_wal_snapshot_without_mutating_source() {
        let db = tempfile::NamedTempFile::new().unwrap();
        let writer = Connection::open(db.path()).unwrap();
        writer
            .execute_batch(
                r#"
PRAGMA journal_mode = WAL;
PRAGMA wal_autocheckpoint = 0;
CREATE TABLE history (
  id TEXT PRIMARY KEY,
  timestamp INTEGER NOT NULL,
  duration INTEGER NOT NULL,
  exit INTEGER NOT NULL,
  command TEXT NOT NULL,
  cwd TEXT NOT NULL,
  session TEXT NOT NULL,
  hostname TEXT NOT NULL,
  deleted_at INTEGER
);
INSERT INTO history VALUES (
  'wal-1', 1700000000000000000, 0, 0, 'echo wal', '/work', 'session', 'host', NULL
);
"#,
            )
            .unwrap();
        let store = LocalStore::open(":memory:").unwrap();

        let stats = run_import(&store, db.path(), None).unwrap();
        assert_eq!(stats.inserted, 1);
        assert_eq!(store.list_recent(10).unwrap()[0].cmd, "echo wal");
        assert_eq!(
            writer
                .query_row("SELECT COUNT(*) FROM history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            writer
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'entries'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn atuin_blank_id_is_stable_across_databases_and_import_devices() {
        let first_db = atuin_db(true, false);
        let second_db = atuin_db(true, false);
        insert_row(
            second_db.path(),
            "deleted-padding",
            1_699_999_999 * SECOND,
            0,
            0,
            "echo padding",
            "/padding",
            "padding-session",
            "padding-host",
            Some(1_700_000_000 * SECOND),
        );
        for path in [first_db.path(), second_db.path()] {
            insert_row(
                path,
                "",
                1_700_000_001 * SECOND,
                SECOND,
                0,
                "echo stable",
                "/work",
                "session",
                "source-host",
                None,
            );
        }
        let store = LocalStore::open(":memory:").unwrap();

        let first = run_import_with_device(&store, first_db.path(), None, "device-a").unwrap();
        let second = run_import_with_device(&store, second_db.path(), None, "device-b").unwrap();
        assert_eq!(first.inserted, 1);
        assert_eq!(second.inserted, 0);
        assert_eq!(second.ignored, 1);
        assert_eq!(store.list_recent(10).unwrap().len(), 1);
        assert_eq!(store.list_recent(10).unwrap()[0].device_id, "device-a");
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn atuin_import_rejects_limit_outside_sqlite_range() {
        let db = atuin_db(true, false);
        let store = LocalStore::open(":memory:").unwrap();
        let error = run_import(&store, db.path(), Some(usize::MAX)).unwrap_err();
        assert!(format!("{error:#}").contains("limit exceeds SQLite range"));
    }

    fn run_import(
        store: &LocalStore,
        path: &std::path::Path,
        limit: Option<usize>,
    ) -> Result<ImportStats> {
        run_import_with_device(store, path, limit, "rustory-device")
    }

    fn run_import_with_device(
        store: &LocalStore,
        path: &std::path::Path,
        limit: Option<usize>,
        device_id: &str,
    ) -> Result<ImportStats> {
        import_path_into_store(
            store,
            HistoryShell::Atuin,
            PathImportRequest {
                path,
                limit,
                user_id: "rustory-user",
                device_id,
                hostname: "fallback-host",
                ignore_regex: None,
            },
        )
    }

    fn atuin_db(has_deleted_at: bool, has_new_metadata: bool) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(
            r#"
CREATE TABLE history (
  id TEXT PRIMARY KEY,
  timestamp INTEGER NOT NULL,
  duration INTEGER NOT NULL,
  exit INTEGER NOT NULL,
  command TEXT NOT NULL,
  cwd TEXT NOT NULL,
  session TEXT NOT NULL,
  hostname TEXT NOT NULL,
  UNIQUE(timestamp, cwd, command)
);
"#,
        )
        .unwrap();
        if has_deleted_at {
            conn.execute_batch("ALTER TABLE history ADD COLUMN deleted_at INTEGER;")
                .unwrap();
        }
        if has_new_metadata {
            conn.execute_batch(
                "ALTER TABLE history ADD COLUMN author TEXT; ALTER TABLE history ADD COLUMN intent TEXT;",
            )
            .unwrap();
        }
        file
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_row(
        path: &std::path::Path,
        id: &str,
        timestamp: i64,
        duration: i64,
        exit: i64,
        command: &str,
        cwd: &str,
        session: &str,
        hostname: &str,
        deleted_at: Option<i64>,
    ) {
        let conn = Connection::open(path).unwrap();
        let has_deleted_at = AtuinSchema::inspect(&conn).unwrap().has_deleted_at;
        if has_deleted_at {
            conn.execute(
                r#"
INSERT INTO history(id, timestamp, duration, exit, command, cwd, session, hostname, deleted_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
"#,
                params![
                    id, timestamp, duration, exit, command, cwd, session, hostname, deleted_at
                ],
            )
            .unwrap();
        } else {
            conn.execute(
                r#"
INSERT INTO history(id, timestamp, duration, exit, command, cwd, session, hostname)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
"#,
                params![
                    id, timestamp, duration, exit, command, cwd, session, hostname
                ],
            )
            .unwrap();
        }
    }
}
