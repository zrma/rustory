use crate::{core, storage::LocalStore};
use anyhow::{Context, Result};
use std::path::Path;
use time::{Duration as TimeDuration, OffsetDateTime};

#[cfg(feature = "import-atuin")]
mod atuin;
#[cfg(feature = "import-hishtory")]
mod hishtory;

#[cfg(all(test, feature = "import-hishtory"))]
pub use hishtory::{HishtoryImportRequest, import_hishtory_sqlite_into_store};

#[cfg(all(feature = "import-atuin", feature = "import-hishtory"))]
pub const IMPORT_SOURCE_HELP: &str =
    "History source format to import: bash, zsh, hishtory, or atuin";
#[cfg(all(feature = "import-atuin", not(feature = "import-hishtory")))]
pub const IMPORT_SOURCE_HELP: &str = "History source format to import: bash, zsh, or atuin";
#[cfg(all(not(feature = "import-atuin"), feature = "import-hishtory"))]
pub const IMPORT_SOURCE_HELP: &str = "History source format to import: bash, zsh, or hishtory";
#[cfg(all(not(feature = "import-atuin"), not(feature = "import-hishtory")))]
pub const IMPORT_SOURCE_HELP: &str = "History source format to import: bash or zsh";

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
    #[cfg(feature = "import-atuin")]
    Atuin,
    Bash,
    #[cfg(feature = "import-hishtory")]
    Hishtory,
    Zsh,
}

impl HistoryShell {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            #[cfg(feature = "import-atuin")]
            "atuin" => Ok(Self::Atuin),
            "bash" => Ok(Self::Bash),
            #[cfg(feature = "import-hishtory")]
            "hishtory" => Ok(Self::Hishtory),
            "zsh" => Ok(Self::Zsh),
            _ => anyhow::bail!(
                "unsupported history source: {value} (enabled: {})",
                enabled_history_sources()
            ),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            #[cfg(feature = "import-atuin")]
            Self::Atuin => "atuin",
            Self::Bash => "bash",
            #[cfg(feature = "import-hishtory")]
            Self::Hishtory => "hishtory",
            Self::Zsh => "zsh",
        }
    }

    pub fn default_history_path(self) -> String {
        match self {
            #[cfg(feature = "import-atuin")]
            Self::Atuin => atuin::default_history_path(),
            Self::Bash => "~/.bash_history".to_string(),
            #[cfg(feature = "import-hishtory")]
            Self::Hishtory => "~/.hishtory/.hishtory.db".to_string(),
            Self::Zsh => "~/.zsh_history".to_string(),
        }
    }
}

fn enabled_history_sources() -> &'static str {
    #[cfg(all(feature = "import-atuin", feature = "import-hishtory"))]
    return "bash|zsh|hishtory|atuin";
    #[cfg(all(feature = "import-atuin", not(feature = "import-hishtory")))]
    return "bash|zsh|atuin";
    #[cfg(all(not(feature = "import-atuin"), feature = "import-hishtory"))]
    return "bash|zsh|hishtory";
    #[cfg(all(not(feature = "import-atuin"), not(feature = "import-hishtory")))]
    return "bash|zsh";
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub received: usize,
    pub inserted: usize,
    pub ignored: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct PathImportRequest<'a> {
    pub path: &'a Path,
    pub limit: Option<usize>,
    pub user_id: &'a str,
    pub device_id: &'a str,
    pub hostname: &'a str,
    pub ignore_regex: Option<&'a regex::Regex>,
}

#[cfg(any(feature = "import-atuin", feature = "import-hishtory"))]
pub(super) trait HistoryImportAdapter: Sync {
    fn import(&self, store: &LocalStore, req: PathImportRequest<'_>) -> Result<ImportStats>;
}

pub fn import_path_into_store(
    store: &LocalStore,
    shell: HistoryShell,
    req: PathImportRequest<'_>,
) -> Result<ImportStats> {
    match shell {
        #[cfg(feature = "import-atuin")]
        HistoryShell::Atuin => atuin::ADAPTER.import(store, req),
        HistoryShell::Bash | HistoryShell::Zsh => {
            let content = read_history_file(req.path)?;
            import_into_store(
                store,
                ImportRequest {
                    shell,
                    content: &content,
                    limit: req.limit,
                    user_id: req.user_id,
                    device_id: req.device_id,
                    hostname: req.hostname,
                    ignore_regex: req.ignore_regex,
                },
            )
        }
        #[cfg(feature = "import-hishtory")]
        HistoryShell::Hishtory => hishtory::ADAPTER.import(store, req),
    }
}

#[cfg(any(feature = "import-atuin", feature = "import-hishtory"))]
pub(super) struct AdapterRecord {
    pub entry_id: core::EntryId,
    pub ts: OffsetDateTime,
    pub duration_ms: i64,
    pub cmd: String,
    pub cwd: String,
    pub exit_code: i32,
    pub hostname: String,
}

#[cfg(any(feature = "import-atuin", feature = "import-hishtory"))]
pub(super) fn insert_adapter_records(
    store: &LocalStore,
    req: PathImportRequest<'_>,
    source: &str,
    records: Vec<AdapterRecord>,
) -> Result<ImportStats> {
    let mut stats = ImportStats::default();
    const BATCH: usize = 2000;
    let mut buf: Vec<core::Entry> = Vec::with_capacity(BATCH);

    for record in records {
        stats.received += 1;
        let cmd = record.cmd.trim();
        if should_skip_import_command(cmd, req.ignore_regex) {
            stats.skipped += 1;
            continue;
        }

        buf.push(core::Entry::new_with_id(
            record.entry_id,
            core::EntryInput {
                device_id: req.device_id.to_string(),
                user_id: req.user_id.to_string(),
                ts: record.ts,
                cmd: cmd.to_string(),
                cwd: normalize_import_text(&record.cwd, "unknown"),
                exit_code: record.exit_code,
                duration_ms: record.duration_ms,
                shell: source.to_string(),
                hostname: normalize_import_text(&record.hostname, req.hostname),
            },
        ));

        if buf.len() >= BATCH {
            let inserted = store.insert_entries_with_stats(&buf)?;
            stats.inserted += inserted.inserted;
            stats.ignored += inserted.ignored;
            buf.clear();
        }
    }

    if !buf.is_empty() {
        let inserted = store.insert_entries_with_stats(&buf)?;
        stats.inserted += inserted.inserted;
        stats.ignored += inserted.ignored;
    }

    Ok(stats)
}

pub fn read_history_file(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read history file: {}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

pub fn parse_history(shell: HistoryShell, content: &str) -> Vec<HistoryRecord> {
    match shell {
        #[cfg(feature = "import-atuin")]
        HistoryShell::Atuin => Vec::new(),
        HistoryShell::Bash => parse_bash_history(content),
        #[cfg(feature = "import-hishtory")]
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
        matches!(req.shell, HistoryShell::Bash | HistoryShell::Zsh),
        "SQLite history sources require import_path_into_store"
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

pub struct ImportRequest<'a> {
    pub shell: HistoryShell,
    pub content: &'a str,
    pub limit: Option<usize>,
    pub user_id: &'a str,
    pub device_id: &'a str,
    pub hostname: &'a str,
    pub ignore_regex: Option<&'a regex::Regex>,
}

#[cfg(any(feature = "import-atuin", feature = "import-hishtory"))]
fn should_skip_import_command(cmd: &str, ignore_regex: Option<&regex::Regex>) -> bool {
    if cmd.is_empty() {
        return true;
    }
    if let Some(regex) = ignore_regex
        && regex.is_match(cmd)
    {
        return true;
    }
    false
}

#[cfg(any(feature = "import-atuin", feature = "import-hishtory"))]
fn normalize_import_text(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalStore;
    #[cfg(feature = "import-hishtory")]
    use rusqlite::{Connection, params};
    #[cfg(feature = "import-hishtory")]
    use std::collections::BTreeSet;

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

    #[cfg(feature = "import-hishtory")]
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
        assert_eq!(s1.inserted, 2);
        assert_eq!(s1.ignored, 0);
        assert_eq!(s1.skipped, 1);

        let entries = store.list_recent(10).unwrap();
        assert_eq!(entries.len(), 2);
        let entry_ids_before_second_import = entries
            .iter()
            .map(|entry| entry.entry_id.clone())
            .collect::<BTreeSet<_>>();
        let echo_entry = entries.iter().find(|entry| entry.cmd == "echo ok").unwrap();
        assert_eq!(echo_entry.device_id, "rustory-device-a");
        assert_eq!(echo_entry.user_id, "u1");
        assert_eq!(echo_entry.shell, "hishtory");
        assert_eq!(echo_entry.hostname, "old-host");
        assert_eq!(echo_entry.cwd, "/work");
        assert_eq!(echo_entry.exit_code, 7);
        assert_eq!(echo_entry.duration_ms, 2000);
        assert_eq!(echo_entry.ts.unix_timestamp(), 1704067200);
        assert!(entries.iter().any(|entry| entry.cmd == "rr doctor"));

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
        assert_eq!(s2.ignored, 2);
        assert_eq!(s2.skipped, 1);
        let entries_after_second_import = store.list_recent(10).unwrap();
        assert_eq!(entries_after_second_import.len(), 2);
        let entry_ids_after_second_import = entries_after_second_import
            .iter()
            .map(|entry| entry.entry_id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            entry_ids_after_second_import,
            entry_ids_before_second_import
        );
    }

    #[cfg(feature = "import-hishtory")]
    #[test]
    fn import_hishtory_sqlite_decodes_invalid_utf8_lossily() {
        let hishtory_db = synthetic_hishtory_db(&[]);
        let conn = Connection::open(hishtory_db.path()).unwrap();
        conn.execute_batch(
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
) VALUES (
  CAST(x'6c6f63616cff' AS TEXT),
  CAST(x'686f7374ff' AS TEXT),
  CAST(x'6563686f20ff' AS TEXT),
  CAST(x'2f776f726bff' AS TEXT),
  CAST(x'2f686f6d65ff' AS TEXT),
  0,
  '2024-01-01 00:00:00+00:00',
  '2024-01-01 00:00:01+00:00',
  CAST(x'646576ff' AS TEXT),
  X'',
  CAST(x'68697374ff' AS TEXT)
);
"#,
        )
        .unwrap();
        drop(conn);
        let store = LocalStore::open(":memory:").unwrap();

        let s1 = import_hishtory_sqlite_into_store(
            &store,
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
        assert_eq!(s1.received, 1);
        assert_eq!(s1.inserted, 1);

        let entries = store.list_recent(10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].cmd,
            String::from_utf8_lossy(b"echo \xff").as_ref()
        );
        assert_eq!(
            entries[0].cwd,
            String::from_utf8_lossy(b"/work\xff").as_ref()
        );
        assert_eq!(
            entries[0].hostname,
            String::from_utf8_lossy(b"host\xff").as_ref()
        );

        let s2 = import_hishtory_sqlite_into_store(
            &store,
            HishtoryImportRequest {
                path: hishtory_db.path(),
                limit: None,
                user_id: "u1",
                device_id: "rustory-device-b",
                hostname: "fallback-host",
                ignore_regex: None,
            },
        )
        .unwrap();
        assert_eq!(s2.received, 1);
        assert_eq!(s2.inserted, 0);
        assert_eq!(s2.ignored, 1);
    }

    #[cfg(feature = "import-hishtory")]
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
        assert_eq!(s2.inserted, 1);
        assert_eq!(s2.ignored, 1);
        assert_eq!(s2.skipped, 0);
        assert_eq!(store.list_recent(10).unwrap().len(), 2);
    }

    #[cfg(feature = "import-hishtory")]
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

    #[cfg(feature = "import-hishtory")]
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
            |entries, deletions| {
                remote.insert_entries(&entries)?;
                remote.apply_entry_deletions_with_stats(&deletions)?;
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

    #[cfg(feature = "import-hishtory")]
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

    #[cfg(feature = "import-hishtory")]
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

#[cfg(test)]
mod feature_contract_tests {
    use super::*;

    #[test]
    fn flat_file_sources_are_always_enabled() {
        assert_eq!(HistoryShell::parse("bash").unwrap(), HistoryShell::Bash);
        assert_eq!(HistoryShell::parse("zsh").unwrap(), HistoryShell::Zsh);
    }

    #[cfg(feature = "import-atuin")]
    #[test]
    fn atuin_feature_registers_adapter() {
        assert_eq!(HistoryShell::parse("atuin").unwrap(), HistoryShell::Atuin);
        assert_eq!(
            HistoryShell::Atuin.default_history_path(),
            atuin::default_history_path_from_xdg(None)
        );
    }

    #[cfg(not(feature = "import-atuin"))]
    #[test]
    fn atuin_source_is_absent_without_feature() {
        assert!(HistoryShell::parse("atuin").is_err());
    }

    #[cfg(feature = "import-hishtory")]
    #[test]
    fn hishtory_feature_registers_adapter() {
        assert_eq!(
            HistoryShell::parse("hishtory").unwrap(),
            HistoryShell::Hishtory
        );
    }

    #[cfg(not(feature = "import-hishtory"))]
    #[test]
    fn hishtory_source_is_absent_without_feature() {
        assert!(HistoryShell::parse("hishtory").is_err());
    }
}
