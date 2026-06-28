use crate::{core, storage::LocalStore};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, params};
use std::path::Path;
use std::time::Duration as StdDuration;
use time::{Duration as TimeDuration, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRecord {
    // 0-based index among "command records" in the source file.
    pub source_index: u64,
    pub ts_unix: Option<i64>,
    pub duration_ms: i64,
    pub cmd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryShell {
    Bash,
    Hishtory,
    Zsh,
}

impl HistoryShell {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "bash" => Ok(Self::Bash),
            "hishtory" => Ok(Self::Hishtory),
            "zsh" => Ok(Self::Zsh),
            _ => anyhow::bail!("unsupported shell: {value} (expected: bash|zsh|hishtory)"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Hishtory => "hishtory",
            Self::Zsh => "zsh",
        }
    }

    pub fn default_history_path(self) -> &'static str {
        match self {
            Self::Bash => "~/.bash_history",
            Self::Hishtory => "~/.hishtory/.hishtory.db",
            Self::Zsh => "~/.zsh_history",
        }
    }

    pub fn is_hishtory(self) -> bool {
        matches!(self, Self::Hishtory)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub received: usize,
    pub inserted: usize,
    pub ignored: usize,
    pub skipped: usize,
}

pub fn read_history_file(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read history file: {}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

pub fn parse_history(shell: HistoryShell, content: &str) -> Vec<HistoryRecord> {
    match shell {
        HistoryShell::Bash => parse_bash_history(content),
        HistoryShell::Hishtory => Vec::new(),
        HistoryShell::Zsh => parse_zsh_history(content),
    }
}

pub fn parse_zsh_history(content: &str) -> Vec<HistoryRecord> {
    // zsh extended history format (when EXTENDED_HISTORY is enabled):
    //   : <epoch>:<duration>;command
    // If a line doesn't match the format, we treat it as "command without timestamp".
    let mut out = Vec::new();
    let mut cmd_index: u64 = 0;
    for raw in content.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }

        let mut ts_unix: Option<i64> = None;
        let mut duration_ms: i64 = 0;
        let mut cmd = line.to_string();

        // Fast-path parse of ": <epoch>:<duration>;cmd"
        if let Some(rest) = line.strip_prefix(": ")
            && let Some((meta, after)) = rest.split_once(';')
            && let Some((ts_s, dur_s)) = meta.split_once(':')
        {
            if let Ok(ts) = ts_s.parse::<i64>() {
                ts_unix = Some(ts);
            }
            if let Ok(dur) = dur_s.parse::<i64>() {
                duration_ms = dur.saturating_mul(1000);
            }
            cmd = after.to_string();
        }

        out.push(HistoryRecord {
            source_index: cmd_index,
            ts_unix,
            duration_ms,
            cmd,
        });
        cmd_index += 1;
    }
    out
}

pub fn parse_bash_history(content: &str) -> Vec<HistoryRecord> {
    // When HISTTIMEFORMAT is set, bash can write timestamps as:
    //   #<epoch>
    //   <command>
    // If a command line doesn't have a preceding timestamp, we treat it as "timestamp missing".
    let mut out = Vec::new();
    let mut next_ts: Option<i64> = None;
    let mut cmd_index: u64 = 0;

    for raw in content.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix('#')
            && let Ok(ts) = rest.parse::<i64>()
        {
            next_ts = Some(ts);
            continue;
        }

        out.push(HistoryRecord {
            source_index: cmd_index,
            ts_unix: next_ts.take(),
            duration_ms: 0,
            cmd: line.to_string(),
        });
        cmd_index += 1;
    }

    out
}

fn filled_ts_unix(total: usize, index: usize, original: Option<i64>, now: OffsetDateTime) -> i64 {
    match original {
        Some(ts) => ts,
        None => {
            // Preserve file order: oldest record gets the earliest synthetic timestamp.
            let delta = (total - 1).saturating_sub(index) as i64;
            (now - TimeDuration::seconds(delta)).unix_timestamp()
        }
    }
}

pub fn import_into_store(store: &LocalStore, req: ImportRequest<'_>) -> Result<ImportStats> {
    anyhow::ensure!(
        !req.shell.is_hishtory(),
        "hishtory imports require import_hishtory_sqlite_into_store"
    );

    let mut records = parse_history(req.shell, req.content);

    // Keep only the last N commands, if requested.
    if let Some(n) = req.limit
        && records.len() > n
    {
        records = records.split_off(records.len() - n);
    }

    let now = OffsetDateTime::now_utc();
    let total = records.len();

    let mut stats = ImportStats::default();

    // Batch inserts for memory/throughput. (Size is arbitrary and can be tuned later.)
    const BATCH: usize = 2000;
    let mut buf: Vec<core::Entry> = Vec::with_capacity(BATCH);

    for (i, r) in records.into_iter().enumerate() {
        stats.received += 1;

        let cmd = r.cmd.trim();
        if cmd.is_empty() {
            stats.skipped += 1;
            continue;
        }
        if cmd.split_whitespace().next() == Some("rr") {
            stats.skipped += 1;
            continue;
        }
        if let Some(re) = req.ignore_regex
            && re.is_match(cmd)
        {
            stats.skipped += 1;
            continue;
        }

        let ts_unix = filled_ts_unix(total, i, r.ts_unix, now);
        let ts = OffsetDateTime::from_unix_timestamp(ts_unix)
            .map_err(|_| anyhow::anyhow!("invalid unix timestamp: {ts_unix}"))?;

        let id_ts_unix = r.ts_unix.unwrap_or(0);
        let entry_id = core::import_entry_id(
            req.user_id,
            req.device_id,
            req.shell.as_str(),
            id_ts_unix,
            cmd,
            r.source_index,
        );

        buf.push(core::Entry::new_with_id(
            entry_id,
            core::EntryInput {
                device_id: req.device_id.to_string(),
                user_id: req.user_id.to_string(),
                ts,
                cmd: cmd.to_string(),
                cwd: "unknown".to_string(),
                exit_code: 0,
                duration_ms: r.duration_ms,
                shell: req.shell.as_str().to_string(),
                hostname: req.hostname.to_string(),
            },
        ));

        if buf.len() >= BATCH {
            let s = store.insert_entries_with_stats(&buf)?;
            stats.inserted += s.inserted;
            stats.ignored += s.ignored;
            buf.clear();
        }
    }

    if !buf.is_empty() {
        let s = store.insert_entries_with_stats(&buf)?;
        stats.inserted += s.inserted;
        stats.ignored += s.ignored;
    }

    Ok(stats)
}

#[derive(Debug, Clone, Copy)]
pub struct HishtoryImportRequest<'a> {
    pub path: &'a Path,
    pub limit: Option<usize>,
    pub user_id: &'a str,
    pub device_id: &'a str,
    pub hostname: &'a str,
    pub ignore_regex: Option<&'a regex::Regex>,
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

pub fn import_hishtory_sqlite_into_store(
    store: &LocalStore,
    req: HishtoryImportRequest<'_>,
) -> Result<ImportStats> {
    let conn = Connection::open_with_flags(
        req.path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open hishtory sqlite read-only: {}", req.path.display()))?;
    conn.busy_timeout(StdDuration::from_secs(5))
        .context("set hishtory sqlite busy_timeout")?;

    let records = read_hishtory_records(&conn, req.limit, req.hostname)?;
    let mut stats = ImportStats::default();

    const BATCH: usize = 2000;
    let mut buf: Vec<core::Entry> = Vec::with_capacity(BATCH);

    for r in records {
        stats.received += 1;

        let cmd = r.cmd.trim();
        if should_skip_import_command(cmd, req.ignore_regex) {
            stats.skipped += 1;
            continue;
        }

        let ts = OffsetDateTime::from_unix_timestamp(r.ts_unix)
            .map_err(|_| anyhow::anyhow!("invalid hishtory unix timestamp: {}", r.ts_unix))?;

        let entry_id = hishtory_import_entry_id(
            req.user_id,
            r.source_entry_id.as_deref(),
            &r.fallback_source_key,
        );

        buf.push(core::Entry::new_with_id(
            entry_id,
            core::EntryInput {
                device_id: req.device_id.to_string(),
                user_id: req.user_id.to_string(),
                ts,
                cmd: cmd.to_string(),
                cwd: normalize_import_text(&r.cwd, "unknown"),
                exit_code: r.exit_code,
                duration_ms: r.duration_ms,
                shell: HistoryShell::Hishtory.as_str().to_string(),
                hostname: normalize_import_text(&r.hostname, req.hostname),
            },
        ));

        if buf.len() >= BATCH {
            let s = store.insert_entries_with_stats(&buf)?;
            stats.inserted += s.inserted;
            stats.ignored += s.ignored;
            buf.clear();
        }
    }

    if !buf.is_empty() {
        let s = store.insert_entries_with_stats(&buf)?;
        stats.inserted += s.inserted;
        stats.ignored += s.ignored;
    }

    Ok(stats)
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
        let mut stmt = conn
            .prepare(sql_limited)
            .context("prepare hishtory limited import query")?;
        let rows = stmt
            .query_map(
                params![fallback_hostname, limit as i64],
                row_to_hishtory_record,
            )
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

    let cmd: String = row.get(2)?;
    let cwd: String = row.get(3)?;
    let exit_code: i32 = row.get(4)?;
    let hostname: String = row.get(7)?;
    let local_username: Option<String> = row.get(8)?;
    let source_hostname: Option<String> = row.get(9)?;
    let source_cwd: Option<String> = row.get(10)?;
    let home_directory: Option<String> = row.get(11)?;
    let start_time: Option<String> = row.get(12)?;
    let end_time: Option<String> = row.get(13)?;
    let source_device_id: Option<String> = row.get(14)?;
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
        source_entry_id: row.get(1)?,
        fallback_source_key,
        ts_unix: start_ts,
        duration_ms,
        cmd,
        cwd,
        exit_code,
        hostname,
    })
}

fn should_skip_import_command(cmd: &str, ignore_regex: Option<&regex::Regex>) -> bool {
    if cmd.is_empty() {
        return true;
    }
    if cmd.split_whitespace().next() == Some("rr") {
        return true;
    }
    if let Some(re) = ignore_regex
        && re.is_match(cmd)
    {
        return true;
    }
    false
}

fn normalize_import_text(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn hishtory_import_entry_id(
    user_id: &str,
    source_entry_id: Option<&str>,
    fallback_source_key: &str,
) -> core::EntryId {
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
    // Hishtory's SQLite unique index is based on these source fields, not on SQLite rowid.
    // Using the same composite shape keeps old rows with blank entry_id stable across machines.
    let exit_code = key.exit_code.map(|v| v.to_string()).unwrap_or_default();
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

#[derive(Debug, Clone, Copy)]
pub struct ImportRequest<'a> {
    pub shell: HistoryShell,
    pub content: &'a str,
    pub limit: Option<usize>,
    pub user_id: &'a str,
    pub device_id: &'a str,
    pub hostname: &'a str,
    pub ignore_regex: Option<&'a regex::Regex>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalStore;
    use rusqlite::params;

    #[test]
    fn parse_zsh_extended_history() {
        let content = ": 1700000000:2;echo hello\n: 1700000001:0;ls -la\n";
        let got = parse_zsh_history(content);
        assert_eq!(
            got,
            vec![
                HistoryRecord {
                    source_index: 0,
                    ts_unix: Some(1700000000),
                    duration_ms: 2000,
                    cmd: "echo hello".to_string(),
                },
                HistoryRecord {
                    source_index: 1,
                    ts_unix: Some(1700000001),
                    duration_ms: 0,
                    cmd: "ls -la".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parse_bash_history_with_timestamps() {
        let content = "#1700000000\necho a\n#1700000001\nls\n";
        let got = parse_bash_history(content);
        assert_eq!(
            got,
            vec![
                HistoryRecord {
                    source_index: 0,
                    ts_unix: Some(1700000000),
                    duration_ms: 0,
                    cmd: "echo a".to_string(),
                },
                HistoryRecord {
                    source_index: 1,
                    ts_unix: Some(1700000001),
                    duration_ms: 0,
                    cmd: "ls".to_string(),
                },
            ]
        );
    }

    #[test]
    fn import_is_idempotent_for_same_content() {
        let store = LocalStore::open(":memory:").unwrap();
        let content = ": 1700000000:0;echo a\n: 1700000001:0;echo b\n";

        let s1 = import_into_store(
            &store,
            ImportRequest {
                shell: HistoryShell::Zsh,
                content,
                limit: None,
                user_id: "u1",
                device_id: "d1",
                hostname: "host",
                ignore_regex: None,
            },
        )
        .unwrap();
        assert_eq!(s1.received, 2);
        assert_eq!(s1.inserted, 2);
        assert_eq!(s1.ignored, 0);

        let s2 = import_into_store(
            &store,
            ImportRequest {
                shell: HistoryShell::Zsh,
                content,
                limit: None,
                user_id: "u1",
                device_id: "d1",
                hostname: "host",
                ignore_regex: None,
            },
        )
        .unwrap();
        assert_eq!(s2.received, 2);
        assert_eq!(s2.inserted, 0);
        assert_eq!(s2.ignored, 2);
    }

    #[test]
    fn import_is_idempotent_even_without_timestamps() {
        let store = LocalStore::open(":memory:").unwrap();
        let content = "echo a\necho b\n";

        let s1 = import_into_store(
            &store,
            ImportRequest {
                shell: HistoryShell::Bash,
                content,
                limit: None,
                user_id: "u1",
                device_id: "d1",
                hostname: "host",
                ignore_regex: None,
            },
        )
        .unwrap();
        assert_eq!(s1.received, 2);
        assert_eq!(s1.inserted, 2);

        let s2 = import_into_store(
            &store,
            ImportRequest {
                shell: HistoryShell::Bash,
                content,
                limit: None,
                user_id: "u1",
                device_id: "d1",
                hostname: "host",
                ignore_regex: None,
            },
        )
        .unwrap();
        assert_eq!(s2.received, 2);
        assert_eq!(s2.inserted, 0);
        assert_eq!(s2.ignored, 2);
    }

    #[test]
    fn import_applies_ignore_regex() {
        let store = LocalStore::open(":memory:").unwrap();
        let content = ": 1700000000:0;echo token=abc\n: 1700000001:0;echo ok\n";
        let re = regex::Regex::new("(?i)token").unwrap();

        let s = import_into_store(
            &store,
            ImportRequest {
                shell: HistoryShell::Zsh,
                content,
                limit: None,
                user_id: "u1",
                device_id: "d1",
                hostname: "host",
                ignore_regex: Some(&re),
            },
        )
        .unwrap();

        assert_eq!(s.received, 2);
        assert_eq!(s.skipped, 1);
        assert_eq!(s.inserted, 1);
        assert_eq!(store.list_recent(10).unwrap().len(), 1);
    }

    #[test]
    fn import_hishtory_sqlite_preserves_metadata_and_is_idempotent() {
        let hishtory_db = synthetic_hishtory_db(&[
            HishtoryFixtureRow {
                entry_id: "hist-1",
                hostname: "old-host",
                command: "echo ok",
                cwd: "/work",
                exit_code: 7,
                start_time: "2024-01-01 00:00:00+00:00",
                end_time: "2024-01-01 00:00:02+00:00",
            },
            HishtoryFixtureRow {
                entry_id: "hist-2",
                hostname: "old-host",
                command: "rr doctor",
                cwd: "/work",
                exit_code: 0,
                start_time: "2024-01-01 00:00:03+00:00",
                end_time: "2024-01-01 00:00:04+00:00",
            },
            HishtoryFixtureRow {
                entry_id: "hist-3",
                hostname: "old-host",
                command: "echo token=abc",
                cwd: "/work",
                exit_code: 0,
                start_time: "2024-01-01 00:00:05+00:00",
                end_time: "2024-01-01 00:00:06+00:00",
            },
        ]);
        let store = LocalStore::open(":memory:").unwrap();
        let ignore_re = regex::Regex::new("token=").unwrap();

        let s1 = import_hishtory_sqlite_into_store(
            &store,
            HishtoryImportRequest {
                path: hishtory_db.path(),
                limit: None,
                user_id: "u1",
                device_id: "rustory-device-a",
                hostname: "fallback-host",
                ignore_regex: Some(&ignore_re),
            },
        )
        .unwrap();
        assert_eq!(s1.received, 3);
        assert_eq!(s1.inserted, 1);
        assert_eq!(s1.ignored, 0);
        assert_eq!(s1.skipped, 2);

        let entries = store.list_recent(10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].device_id, "rustory-device-a");
        assert_eq!(entries[0].user_id, "u1");
        assert_eq!(entries[0].shell, "hishtory");
        assert_eq!(entries[0].hostname, "old-host");
        assert_eq!(entries[0].cmd, "echo ok");
        assert_eq!(entries[0].cwd, "/work");
        assert_eq!(entries[0].exit_code, 7);
        assert_eq!(entries[0].duration_ms, 2000);
        assert_eq!(entries[0].ts.unix_timestamp(), 1704067200);

        let s2 = import_hishtory_sqlite_into_store(
            &store,
            HishtoryImportRequest {
                path: hishtory_db.path(),
                limit: None,
                user_id: "u1",
                device_id: "rustory-device-b",
                hostname: "fallback-host",
                ignore_regex: Some(&ignore_re),
            },
        )
        .unwrap();
        assert_eq!(s2.received, 3);
        assert_eq!(s2.inserted, 0);
        assert_eq!(s2.ignored, 1);
        assert_eq!(s2.skipped, 2);
        assert_eq!(store.list_recent(10).unwrap().len(), 1);
    }

    #[test]
    fn import_hishtory_sqlite_blank_entry_id_uses_stable_composite_key() {
        let first_db = synthetic_hishtory_db(&[HishtoryFixtureRow {
            entry_id: "",
            hostname: "",
            command: "echo old-row",
            cwd: "/work",
            exit_code: 0,
            start_time: "2023-01-01 00:00:00.123456+00:00",
            end_time: "2023-01-01 00:00:01.123456+00:00",
        }]);
        let second_db = synthetic_hishtory_db(&[
            HishtoryFixtureRow {
                entry_id: "",
                hostname: "",
                command: "rr doctor",
                cwd: "/work",
                exit_code: 0,
                start_time: "2023-01-01 00:00:00.000001+00:00",
                end_time: "2023-01-01 00:00:00.000002+00:00",
            },
            HishtoryFixtureRow {
                entry_id: "",
                hostname: "",
                command: "echo old-row",
                cwd: "/work",
                exit_code: 0,
                start_time: "2023-01-01 00:00:00.123456+00:00",
                end_time: "2023-01-01 00:00:01.123456+00:00",
            },
        ]);
        let store = LocalStore::open(":memory:").unwrap();

        let s1 = import_hishtory_sqlite_into_store(
            &store,
            HishtoryImportRequest {
                path: first_db.path(),
                limit: None,
                user_id: "u1",
                device_id: "rustory-device-a",
                hostname: "fallback-host",
                ignore_regex: None,
            },
        )
        .unwrap();
        assert_eq!(s1.received, 1);
        assert_eq!(s1.inserted, 1);

        let s2 = import_hishtory_sqlite_into_store(
            &store,
            HishtoryImportRequest {
                path: second_db.path(),
                limit: None,
                user_id: "u1",
                device_id: "rustory-device-b",
                hostname: "different-fallback-host",
                ignore_regex: None,
            },
        )
        .unwrap();
        assert_eq!(s2.received, 2);
        assert_eq!(s2.inserted, 0);
        assert_eq!(s2.ignored, 1);
        assert_eq!(s2.skipped, 1);
        assert_eq!(store.list_recent(10).unwrap().len(), 1);
    }

    #[test]
    fn import_hishtory_sqlite_limit_keeps_newest_rows() {
        let hishtory_db = synthetic_hishtory_db(&[
            HishtoryFixtureRow {
                entry_id: "hist-1",
                hostname: "host",
                command: "echo one",
                cwd: "/one",
                exit_code: 0,
                start_time: "2024-01-01 00:00:00+00:00",
                end_time: "2024-01-01 00:00:01+00:00",
            },
            HishtoryFixtureRow {
                entry_id: "hist-2",
                hostname: "host",
                command: "echo two",
                cwd: "/two",
                exit_code: 0,
                start_time: "2024-01-01 00:00:02+00:00",
                end_time: "2024-01-01 00:00:03+00:00",
            },
            HishtoryFixtureRow {
                entry_id: "hist-3",
                hostname: "host",
                command: "echo three",
                cwd: "/three",
                exit_code: 0,
                start_time: "2024-01-01 00:00:04+00:00",
                end_time: "2024-01-01 00:00:05+00:00",
            },
        ]);
        let store = LocalStore::open(":memory:").unwrap();

        let stats = import_hishtory_sqlite_into_store(
            &store,
            HishtoryImportRequest {
                path: hishtory_db.path(),
                limit: Some(2),
                user_id: "u1",
                device_id: "d1",
                hostname: "fallback-host",
                ignore_regex: None,
            },
        )
        .unwrap();

        assert_eq!(stats.received, 2);
        assert_eq!(stats.inserted, 2);

        let commands: Vec<_> = store
            .list_recent(10)
            .unwrap()
            .into_iter()
            .map(|entry| entry.cmd)
            .collect();
        assert_eq!(commands, vec!["echo three", "echo two"]);
    }

    #[test]
    fn import_hishtory_sqlite_entries_are_pushable_for_import_device() {
        let hishtory_db = synthetic_hishtory_db(&[HishtoryFixtureRow {
            entry_id: "hist-1",
            hostname: "old-host",
            command: "echo migrate-me",
            cwd: "/work",
            exit_code: 0,
            start_time: "2024-01-01 00:00:00+00:00",
            end_time: "2024-01-01 00:00:01+00:00",
        }]);
        let local = LocalStore::open(":memory:").unwrap();
        let remote = LocalStore::open(":memory:").unwrap();

        let stats = import_hishtory_sqlite_into_store(
            &local,
            HishtoryImportRequest {
                path: hishtory_db.path(),
                limit: None,
                user_id: "u1",
                device_id: "rustory-device-a",
                hostname: "fallback-host",
                ignore_regex: None,
            },
        )
        .unwrap();
        assert_eq!(stats.inserted, 1);

        let pushed = crate::sync::sync_push_to_peer(
            &local,
            "peer-1",
            100,
            Some("rustory-device-a"),
            |entries| {
                remote.insert_entries(&entries)?;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(pushed, 1);

        let remote_entries = remote.list_recent(10).unwrap();
        assert_eq!(remote_entries.len(), 1);
        assert_eq!(remote_entries[0].cmd, "echo migrate-me");
        assert_eq!(remote_entries[0].device_id, "rustory-device-a");
    }

    #[derive(Clone, Copy)]
    struct HishtoryFixtureRow<'a> {
        entry_id: &'a str,
        hostname: &'a str,
        command: &'a str,
        cwd: &'a str,
        exit_code: i32,
        start_time: &'a str,
        end_time: &'a str,
    }

    fn synthetic_hishtory_db(rows: &[HishtoryFixtureRow<'_>]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        let conn = Connection::open(file.path()).unwrap();
        conn.execute_batch(
            r#"
CREATE TABLE history_entries (
  local_username text,
  hostname text,
  command text,
  current_working_directory text,
  home_directory text,
  exit_code integer,
  start_time datetime,
  end_time datetime,
  device_id text,
  custom_columns blob,
  entry_id text
);
"#,
        )
        .unwrap();

        for row in rows {
            conn.execute(
                r#"
INSERT INTO history_entries (
  local_username,
  hostname,
  command,
  current_working_directory,
  home_directory,
  exit_code,
  start_time,
  end_time,
  device_id,
  custom_columns,
  entry_id
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#,
                params![
                    "local-user",
                    row.hostname,
                    row.command,
                    row.cwd,
                    "/home/local-user",
                    row.exit_code,
                    row.start_time,
                    row.end_time,
                    "hishtory-device",
                    Vec::<u8>::new(),
                    row.entry_id,
                ],
            )
            .unwrap();
        }
        drop(conn);
        file
    }
}
