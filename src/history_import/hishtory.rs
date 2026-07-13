use super::{
    AdapterRecord, HistoryImportAdapter, ImportStats, PathImportRequest, insert_adapter_records,
    sqlite::{row_lossy_opt_string, row_lossy_string},
};
use crate::storage::LocalStore;
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, params};
use std::time::Duration as StdDuration;
use time::OffsetDateTime;

pub(super) static ADAPTER: HishtoryAdapter = HishtoryAdapter;

#[cfg(test)]
pub type HishtoryImportRequest<'a> = PathImportRequest<'a>;

#[cfg(test)]
pub fn import_hishtory_sqlite_into_store(
    store: &LocalStore,
    req: HishtoryImportRequest<'_>,
) -> Result<ImportStats> {
    ADAPTER.import(store, req)
}

pub(super) struct HishtoryAdapter;

impl HistoryImportAdapter for HishtoryAdapter {
    fn import(&self, store: &LocalStore, req: PathImportRequest<'_>) -> Result<ImportStats> {
        let conn = Connection::open_with_flags(
            req.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("open hishtory sqlite read-only: {}", req.path.display()))?;
        conn.busy_timeout(StdDuration::from_secs(5))
            .context("set hishtory sqlite busy_timeout")?;

        let records = read_hishtory_records(&conn, req.limit, req.hostname)?
            .into_iter()
            .map(|record| {
                let ts = OffsetDateTime::from_unix_timestamp(record.ts_unix).map_err(|_| {
                    anyhow::anyhow!("invalid hishtory unix timestamp: {}", record.ts_unix)
                })?;
                Ok(AdapterRecord {
                    entry_id: hishtory_import_entry_id(
                        req.user_id,
                        record.source_entry_id.as_deref(),
                        &record.fallback_source_key,
                    ),
                    ts,
                    duration_ms: record.duration_ms,
                    cmd: record.cmd,
                    cwd: record.cwd,
                    exit_code: record.exit_code,
                    hostname: record.hostname,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        insert_adapter_records(store, req, "hishtory", records)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HishtoryRecord {
    source_entry_id: Option<String>,
    fallback_source_key: String,
    ts_unix: i64,
    duration_ms: i64,
    cmd: String,
    cwd: String,
    exit_code: i32,
    hostname: String,
}

fn read_hishtory_records(
    conn: &Connection,
    limit: Option<usize>,
    fallback_hostname: &str,
) -> Result<Vec<HishtoryRecord>> {
    let sql_all = r#"
SELECT
  rowid AS source_rowid,
  NULLIF(TRIM(COALESCE(entry_id, '')), '') AS source_entry_id,
  command,
  COALESCE(NULLIF(current_working_directory, ''), 'unknown') AS cwd,
  COALESCE(exit_code, 0) AS exit_code,
  unixepoch(start_time) AS start_ts,
  unixepoch(end_time) AS end_ts,
  COALESCE(NULLIF(hostname, ''), ?1) AS hostname,
  local_username,
  hostname AS source_hostname,
  current_working_directory AS source_cwd,
  home_directory,
  start_time,
  end_time,
  device_id,
  exit_code AS source_exit_code
FROM history_entries
WHERE command IS NOT NULL
  AND TRIM(command) != ''
  AND unixepoch(start_time) IS NOT NULL
ORDER BY start_ts ASC, source_rowid ASC
"#;

    let sql_limited = r#"
SELECT
  source_rowid,
  source_entry_id,
  command,
  cwd,
  exit_code,
  start_ts,
  end_ts,
  hostname,
  local_username,
  source_hostname,
  source_cwd,
  home_directory,
  start_time,
  end_time,
  source_device_id,
  source_exit_code
FROM (
  SELECT
    rowid AS source_rowid,
    NULLIF(TRIM(COALESCE(entry_id, '')), '') AS source_entry_id,
    command,
    COALESCE(NULLIF(current_working_directory, ''), 'unknown') AS cwd,
    COALESCE(exit_code, 0) AS exit_code,
    unixepoch(start_time) AS start_ts,
    unixepoch(end_time) AS end_ts,
    COALESCE(NULLIF(hostname, ''), ?1) AS hostname,
    local_username,
    hostname AS source_hostname,
    current_working_directory AS source_cwd,
    home_directory,
    start_time,
    end_time,
    device_id AS source_device_id,
    exit_code AS source_exit_code
  FROM history_entries
  WHERE command IS NOT NULL
    AND TRIM(command) != ''
    AND unixepoch(start_time) IS NOT NULL
  ORDER BY start_ts DESC, source_rowid DESC
  LIMIT ?2
) selected
ORDER BY start_ts ASC, source_rowid ASC
"#;

    let mut out = Vec::new();
    if let Some(limit) = limit {
        let limit = i64::try_from(limit).context("hishtory import limit exceeds SQLite range")?;
        let mut stmt = conn
            .prepare(sql_limited)
            .context("prepare hishtory limited import query")?;
        let rows = stmt
            .query_map(params![fallback_hostname, limit], row_to_hishtory_record)
            .context("query hishtory limited records")?;
        for row in rows {
            out.push(row?);
        }
    } else {
        let mut stmt = conn
            .prepare(sql_all)
            .context("prepare hishtory import query")?;
        let rows = stmt
            .query_map(params![fallback_hostname], row_to_hishtory_record)
            .context("query hishtory records")?;
        for row in rows {
            out.push(row?);
        }
    }

    Ok(out)
}

fn row_to_hishtory_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<HishtoryRecord> {
    let start_ts: i64 = row.get(5)?;
    let end_ts: Option<i64> = row.get(6)?;
    let duration_ms = end_ts
        .filter(|end| *end > start_ts)
        .map(|end| end.saturating_sub(start_ts).saturating_mul(1000))
        .unwrap_or(0);

    let cmd = row_lossy_string(row, 2)?;
    let cwd = row_lossy_string(row, 3)?;
    let exit_code: i32 = row.get(4)?;
    let hostname = row_lossy_string(row, 7)?;
    let local_username = row_lossy_opt_string(row, 8)?;
    let source_hostname = row_lossy_opt_string(row, 9)?;
    let source_cwd = row_lossy_opt_string(row, 10)?;
    let home_directory = row_lossy_opt_string(row, 11)?;
    let start_time = row_lossy_opt_string(row, 12)?;
    let end_time = row_lossy_opt_string(row, 13)?;
    let source_device_id = row_lossy_opt_string(row, 14)?;
    let source_exit_code: Option<i64> = row.get(15)?;
    let fallback_source_key = hishtory_composite_source_key(HishtoryCompositeSourceKey {
        local_username: local_username.as_deref(),
        hostname: source_hostname.as_deref(),
        cmd: &cmd,
        cwd: source_cwd.as_deref(),
        home_directory: home_directory.as_deref(),
        exit_code: source_exit_code,
        start_time: start_time.as_deref(),
        end_time: end_time.as_deref(),
        source_device_id: source_device_id.as_deref(),
    });

    Ok(HishtoryRecord {
        source_entry_id: row_lossy_opt_string(row, 1)?,
        fallback_source_key,
        ts_unix: start_ts,
        duration_ms,
        cmd,
        cwd,
        exit_code,
        hostname,
    })
}

fn hishtory_import_entry_id(
    user_id: &str,
    source_entry_id: Option<&str>,
    fallback_source_key: &str,
) -> crate::core::EntryId {
    let source_key = match source_entry_id.and_then(|id| {
        let trimmed = id.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }) {
        Some(id) => format!("entry_id\0{id}"),
        None => format!("fallback\0{fallback_source_key}"),
    };
    let name = format!("rustory:hishtory-import\0{user_id}\0{source_key}");
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, name.as_bytes()).to_string()
}

struct HishtoryCompositeSourceKey<'a> {
    local_username: Option<&'a str>,
    hostname: Option<&'a str>,
    cmd: &'a str,
    cwd: Option<&'a str>,
    home_directory: Option<&'a str>,
    exit_code: Option<i64>,
    start_time: Option<&'a str>,
    end_time: Option<&'a str>,
    source_device_id: Option<&'a str>,
}

fn hishtory_composite_source_key(key: HishtoryCompositeSourceKey<'_>) -> String {
    let exit_code = key
        .exit_code
        .map(|value| value.to_string())
        .unwrap_or_default();
    let parts = [
        key.local_username.unwrap_or(""),
        key.hostname.unwrap_or(""),
        key.cmd,
        key.cwd.unwrap_or(""),
        key.home_directory.unwrap_or(""),
        key.start_time.unwrap_or(""),
        key.end_time.unwrap_or(""),
        key.source_device_id.unwrap_or(""),
    ];
    format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        parts[0], parts[1], parts[2], parts[3], parts[4], exit_code, parts[5], parts[6], parts[7]
    )
}
