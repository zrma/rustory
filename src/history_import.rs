use crate::{core, storage::LocalStore};
use anyhow::{Context, Result};
use std::path::Path;
use time::{Duration, OffsetDateTime};

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
    Zsh,
}

impl HistoryShell {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "bash" => Ok(Self::Bash),
            "zsh" => Ok(Self::Zsh),
            _ => anyhow::bail!("unsupported shell: {value} (expected: bash|zsh)"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
        }
    }

    pub fn default_history_path(self) -> &'static str {
        match self {
            Self::Bash => "~/.bash_history",
            Self::Zsh => "~/.zsh_history",
        }
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
            (now - Duration::seconds(delta)).unix_timestamp()
        }
    }
}

pub fn import_into_store(store: &LocalStore, req: ImportRequest<'_>) -> Result<ImportStats> {
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
}
