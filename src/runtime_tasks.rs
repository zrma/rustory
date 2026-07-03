use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{config, storage};

pub(crate) const DEFAULT_ASYNC_UPLOAD_INTERVAL_SEC: u64 = 15;
pub(crate) const DEFAULT_ASYNC_UPLOAD_LIMIT: usize = 200;
pub(crate) const DEFAULT_ASYNC_UPLOAD_MARKER_PATH: &str = "~/.config/rustory/async-upload.last";
pub(crate) const DEFAULT_AUTO_PRUNE_DAYS: u64 = 180;
pub(crate) const DEFAULT_AUTO_PRUNE_INTERVAL_SEC: u64 = 86_400;
pub(crate) const DEFAULT_AUTO_PRUNE_KEEP_RECENT: usize = 0;
pub(crate) const DEFAULT_AUTO_PRUNE_MARKER_PATH: &str = "~/.config/rustory/auto-prune.last";
pub(crate) const DEFAULT_AUTO_TOMBSTONE_GC_DAYS: u64 = 90;
pub(crate) const DEFAULT_AUTO_TOMBSTONE_GC_INTERVAL_SEC: u64 = 86_400;
pub(crate) const DEFAULT_AUTO_TOMBSTONE_GC_MARKER_PATH: &str =
    "~/.config/rustory/auto-tombstone-gc.last";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AsyncUploadRuntimeSettings {
    pub(crate) enabled: bool,
    pub(crate) interval_sec: u64,
    pub(crate) limit: usize,
    pub(crate) marker_path: PathBuf,
    pub(crate) last_trigger_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutoPruneRuntimeSettings {
    pub(crate) enabled: bool,
    pub(crate) older_than_days: u64,
    pub(crate) interval_sec: u64,
    pub(crate) keep_recent: usize,
    pub(crate) marker_path: PathBuf,
    pub(crate) last_trigger_unix: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct AutoTombstoneGcRuntimeSettings {
    pub(crate) enabled: bool,
    pub(crate) older_than_days: u64,
    pub(crate) interval_sec: u64,
    pub(crate) marker_path: PathBuf,
    pub(crate) last_trigger_unix: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct AsyncUploadDoctorReport {
    pub(crate) enabled: bool,
    pub(crate) interval_sec: u64,
    pub(crate) limit: usize,
    pub(crate) marker_path: PathBuf,
    pub(crate) last_trigger_unix: Option<i64>,
    pub(crate) next_due_in_sec: u64,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct AutoTombstoneGcDoctorReport {
    pub(crate) enabled: bool,
    pub(crate) older_than_days: u64,
    pub(crate) interval_sec: u64,
    pub(crate) marker_path: PathBuf,
    pub(crate) last_trigger_unix: Option<i64>,
    pub(crate) next_due_in_sec: u64,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub(crate) struct AutoPruneDoctorReport {
    pub(crate) enabled: bool,
    pub(crate) older_than_days: u64,
    pub(crate) interval_sec: u64,
    pub(crate) keep_recent: usize,
    pub(crate) marker_path: PathBuf,
    pub(crate) last_trigger_unix: Option<i64>,
    pub(crate) next_due_in_sec: u64,
}

pub(crate) fn load_async_upload_runtime_settings(
    cfg: &config::FileConfig,
) -> Result<AsyncUploadRuntimeSettings> {
    let marker_path_raw = resolve_async_upload_marker_path(cfg);
    let marker_path = config::expand_home_path(&marker_path_raw)
        .with_context(|| format!("expand async upload marker path: {marker_path_raw}"))?;

    Ok(AsyncUploadRuntimeSettings {
        enabled: resolve_async_upload_enabled(cfg)?,
        interval_sec: resolve_async_upload_interval_sec(cfg)?,
        limit: resolve_async_upload_limit(cfg)?,
        last_trigger_unix: read_rate_limit_marker(&marker_path)?,
        marker_path,
    })
}

pub(crate) fn summarize_async_upload_runtime(
    settings: AsyncUploadRuntimeSettings,
    now_unix: i64,
) -> AsyncUploadDoctorReport {
    AsyncUploadDoctorReport {
        enabled: settings.enabled,
        interval_sec: settings.interval_sec,
        limit: settings.limit,
        marker_path: settings.marker_path,
        last_trigger_unix: settings.last_trigger_unix,
        next_due_in_sec: compute_next_due_in_sec(
            now_unix,
            settings.last_trigger_unix,
            settings.interval_sec,
        ),
    }
}

pub(crate) fn load_auto_prune_runtime_settings(
    cfg: &config::FileConfig,
) -> Result<AutoPruneRuntimeSettings> {
    let marker_path_raw = resolve_auto_prune_marker_path(cfg);
    let marker_path = config::expand_home_path(&marker_path_raw)
        .with_context(|| format!("expand auto prune marker path: {marker_path_raw}"))?;

    Ok(AutoPruneRuntimeSettings {
        enabled: resolve_auto_prune_enabled(cfg)?,
        older_than_days: resolve_auto_prune_days(cfg)?,
        interval_sec: resolve_auto_prune_interval_sec(cfg)?,
        keep_recent: resolve_auto_prune_keep_recent(cfg)?,
        last_trigger_unix: read_rate_limit_marker(&marker_path)?,
        marker_path,
    })
}

pub(crate) fn summarize_auto_prune_runtime(
    settings: AutoPruneRuntimeSettings,
    now_unix: i64,
) -> AutoPruneDoctorReport {
    AutoPruneDoctorReport {
        enabled: settings.enabled,
        older_than_days: settings.older_than_days,
        interval_sec: settings.interval_sec,
        keep_recent: settings.keep_recent,
        marker_path: settings.marker_path,
        last_trigger_unix: settings.last_trigger_unix,
        next_due_in_sec: compute_next_due_in_sec(
            now_unix,
            settings.last_trigger_unix,
            settings.interval_sec,
        ),
    }
}

pub(crate) fn load_auto_tombstone_gc_runtime_settings(
    cfg: &config::FileConfig,
) -> Result<AutoTombstoneGcRuntimeSettings> {
    let marker_path_raw = resolve_auto_tombstone_gc_marker_path(cfg);
    let marker_path = config::expand_home_path(&marker_path_raw)
        .with_context(|| format!("expand auto tombstone gc marker path: {marker_path_raw}"))?;

    Ok(AutoTombstoneGcRuntimeSettings {
        enabled: resolve_auto_tombstone_gc_enabled(cfg)?,
        older_than_days: resolve_auto_tombstone_gc_days(cfg)?,
        interval_sec: resolve_auto_tombstone_gc_interval_sec(cfg)?,
        last_trigger_unix: read_rate_limit_marker(&marker_path)?,
        marker_path,
    })
}

pub(crate) fn summarize_auto_tombstone_gc_runtime(
    settings: AutoTombstoneGcRuntimeSettings,
    now_unix: i64,
) -> AutoTombstoneGcDoctorReport {
    AutoTombstoneGcDoctorReport {
        enabled: settings.enabled,
        older_than_days: settings.older_than_days,
        interval_sec: settings.interval_sec,
        marker_path: settings.marker_path,
        last_trigger_unix: settings.last_trigger_unix,
        next_due_in_sec: compute_next_due_in_sec(
            now_unix,
            settings.last_trigger_unix,
            settings.interval_sec,
        ),
    }
}

pub(crate) fn compute_next_due_in_sec(
    now_unix: i64,
    last_trigger_unix: Option<i64>,
    interval_sec: u64,
) -> u64 {
    let Some(last) = last_trigger_unix else {
        return 0;
    };

    let interval_i64 = i64::try_from(interval_sec).unwrap_or(i64::MAX);
    let elapsed_i64 = now_unix.saturating_sub(last).max(0);
    let remaining_i64 = interval_i64.saturating_sub(elapsed_i64).max(0);
    u64::try_from(remaining_i64).unwrap_or(0)
}

pub(crate) fn maybe_spawn_async_upload(db_path: &str, cfg: &config::FileConfig) -> Result<()> {
    if !resolve_async_upload_enabled(cfg)? {
        return Ok(());
    }

    let min_interval_sec = resolve_async_upload_interval_sec(cfg)?;
    let limit = resolve_async_upload_limit(cfg)?;
    let marker_path = config::expand_home_path(&resolve_async_upload_marker_path(cfg))?;

    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    let last_trigger_unix = read_rate_limit_marker(&marker_path)?;
    if !should_trigger_interval(now_unix, last_trigger_unix, min_interval_sec) {
        return Ok(());
    }
    write_rate_limit_marker(&marker_path, now_unix)?;

    let exe = std::env::current_exe().context("resolve current executable for async upload")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--db-path")
        .arg(db_path)
        .arg("p2p-sync")
        .arg("--push")
        .arg("--limit")
        .arg(limit.to_string())
        .env("RUSTORY_ASYNC_UPLOAD", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn().context("spawn async upload p2p-sync")?;

    Ok(())
}

pub(crate) fn maybe_run_auto_prune(
    store: &storage::LocalStore,
    cfg: &config::FileConfig,
) -> Result<()> {
    if !resolve_auto_prune_enabled(cfg)? {
        return Ok(());
    }

    let older_than_days = resolve_auto_prune_days(cfg)?;
    let keep_recent = resolve_auto_prune_keep_recent(cfg)?;
    let min_interval_sec = resolve_auto_prune_interval_sec(cfg)?;
    let marker_path = config::expand_home_path(&resolve_auto_prune_marker_path(cfg))?;

    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    let last_trigger_unix = read_rate_limit_marker(&marker_path)?;
    if !should_trigger_interval(now_unix, last_trigger_unix, min_interval_sec) {
        return Ok(());
    }

    let cutoff_unix = compute_prune_cutoff_unix(now_unix, older_than_days)?;
    let stats = store.prune_entries_older_than(cutoff_unix, keep_recent, false)?;
    write_rate_limit_marker(&marker_path, now_unix)?;

    if stats.deleted > 0 {
        eprintln!(
            "info: auto prune deleted={} older_than_days={} keep_recent={} cutoff_unix={}",
            stats.deleted, older_than_days, keep_recent, cutoff_unix
        );
    }

    Ok(())
}

pub(crate) fn maybe_run_auto_tombstone_gc(
    store: &storage::LocalStore,
    cfg: &config::FileConfig,
) -> Result<()> {
    if !resolve_auto_tombstone_gc_enabled(cfg)? {
        return Ok(());
    }

    let older_than_days = resolve_auto_tombstone_gc_days(cfg)?;
    let min_interval_sec = resolve_auto_tombstone_gc_interval_sec(cfg)?;
    let marker_path = config::expand_home_path(&resolve_auto_tombstone_gc_marker_path(cfg))?;

    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    let last_trigger_unix = read_rate_limit_marker(&marker_path)?;
    if !should_trigger_interval(now_unix, last_trigger_unix, min_interval_sec) {
        return Ok(());
    }

    let cutoff_unix = compute_prune_cutoff_unix(now_unix, older_than_days)?;
    let stats = store.gc_tombstones_older_than(cutoff_unix, false)?;
    write_rate_limit_marker(&marker_path, now_unix)?;

    if stats.deleted > 0 {
        eprintln!(
            "info: auto tombstone gc deleted={} older_than_days={} cutoff_unix={}",
            stats.deleted, older_than_days, cutoff_unix
        );
    }

    Ok(())
}

pub(crate) fn resolve_bool_setting(
    env_key: &str,
    env_value: Option<String>,
    cfg_value: Option<bool>,
    default: bool,
) -> Result<bool> {
    match env_value {
        Some(raw) => parse_env_bool(&raw, env_key),
        None => Ok(cfg_value.unwrap_or(default)),
    }
}

pub(crate) fn resolve_u64_setting(
    env_key: &str,
    cfg_key: &str,
    env_value: Option<String>,
    cfg_value: Option<u64>,
    default: u64,
    min: u64,
) -> Result<u64> {
    if let Some(raw) = env_value {
        let parsed: u64 = raw
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid {env_key}={:?}: {e}", raw.trim()))?;
        if parsed < min {
            anyhow::bail!("{env_key} must be >= {min}");
        }
        return Ok(parsed);
    }

    let value = cfg_value.unwrap_or(default);
    if value < min {
        anyhow::bail!("{cfg_key} must be >= {min}");
    }
    Ok(value)
}

pub(crate) fn resolve_usize_setting(
    env_key: &str,
    cfg_key: &str,
    env_value: Option<String>,
    cfg_value: Option<usize>,
    default: usize,
    min: usize,
) -> Result<usize> {
    if let Some(raw) = env_value {
        let parsed: usize = raw
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid {env_key}={:?}: {e}", raw.trim()))?;
        if parsed < min {
            anyhow::bail!("{env_key} must be >= {min}");
        }
        return Ok(parsed);
    }

    let value = cfg_value.unwrap_or(default);
    if value < min {
        anyhow::bail!("{cfg_key} must be >= {min}");
    }
    Ok(value)
}

pub(crate) fn resolve_string_setting(
    env_value: Option<String>,
    cfg_value: Option<String>,
    default: &str,
) -> String {
    env_value
        .or_else(|| normalize_opt_string(cfg_value))
        .unwrap_or_else(|| default.to_string())
}

pub(crate) fn parse_env_bool(value: &str, label: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => {
            anyhow::bail!("invalid {label}={value:?}; expected one of 1/0/true/false/yes/no/on/off")
        }
    }
}

pub(crate) fn read_rate_limit_marker(path: &Path) -> Result<Option<i64>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("read rate limit marker: {}", path.display()));
        }
    };

    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let parsed = trimmed
        .parse::<i64>()
        .map_err(|e| anyhow::anyhow!("invalid rate limit marker {}: {e}", path.display()))?;
    Ok(Some(parsed))
}

pub(crate) fn write_rate_limit_marker(path: &Path, now_unix: i64) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create rate limit marker dir: {}", parent.display()))?;
    }
    std::fs::write(path, format!("{now_unix}\n"))
        .with_context(|| format!("write rate limit marker: {}", path.display()))?;
    Ok(())
}

pub(crate) fn should_trigger_interval(
    now_unix: i64,
    last_trigger_unix: Option<i64>,
    min_interval_sec: u64,
) -> bool {
    let min_interval_sec = i64::try_from(min_interval_sec).unwrap_or(i64::MAX);
    let Some(last) = last_trigger_unix else {
        return true;
    };
    now_unix.saturating_sub(last) >= min_interval_sec
}

pub(crate) fn compute_prune_cutoff_unix(now_unix: i64, older_than_days: u64) -> Result<i64> {
    if older_than_days == 0 {
        anyhow::bail!("--older-than-days must be >= 1");
    }

    let retention_sec = i64::try_from(older_than_days)
        .context("older-than-days is too large")?
        .checked_mul(86_400)
        .context("older-than-days is too large")?;

    now_unix
        .checked_sub(retention_sec)
        .context("failed to compute prune cutoff")
}

fn resolve_async_upload_enabled(cfg: &config::FileConfig) -> Result<bool> {
    resolve_bool_setting(
        "RUSTORY_ASYNC_UPLOAD",
        env_nonempty("RUSTORY_ASYNC_UPLOAD"),
        cfg.async_upload,
        false,
    )
}

fn resolve_async_upload_interval_sec(cfg: &config::FileConfig) -> Result<u64> {
    resolve_u64_setting(
        "RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC",
        "async_upload_interval_sec",
        env_nonempty("RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC"),
        cfg.async_upload_interval_sec,
        DEFAULT_ASYNC_UPLOAD_INTERVAL_SEC,
        1,
    )
}

fn resolve_async_upload_limit(cfg: &config::FileConfig) -> Result<usize> {
    resolve_usize_setting(
        "RUSTORY_ASYNC_UPLOAD_LIMIT",
        "async_upload_limit",
        env_nonempty("RUSTORY_ASYNC_UPLOAD_LIMIT"),
        cfg.async_upload_limit,
        DEFAULT_ASYNC_UPLOAD_LIMIT,
        1,
    )
}

fn resolve_async_upload_marker_path(cfg: &config::FileConfig) -> String {
    resolve_string_setting(
        env_nonempty("RUSTORY_ASYNC_UPLOAD_MARKER_PATH"),
        cfg.async_upload_marker_path.clone(),
        DEFAULT_ASYNC_UPLOAD_MARKER_PATH,
    )
}

fn resolve_auto_prune_enabled(cfg: &config::FileConfig) -> Result<bool> {
    resolve_bool_setting(
        "RUSTORY_AUTO_PRUNE",
        env_nonempty("RUSTORY_AUTO_PRUNE"),
        cfg.auto_prune,
        false,
    )
}

fn resolve_auto_prune_days(cfg: &config::FileConfig) -> Result<u64> {
    resolve_u64_setting(
        "RUSTORY_AUTO_PRUNE_DAYS",
        "auto_prune_days",
        env_nonempty("RUSTORY_AUTO_PRUNE_DAYS"),
        cfg.auto_prune_days,
        DEFAULT_AUTO_PRUNE_DAYS,
        1,
    )
}

fn resolve_auto_prune_interval_sec(cfg: &config::FileConfig) -> Result<u64> {
    resolve_u64_setting(
        "RUSTORY_AUTO_PRUNE_INTERVAL_SEC",
        "auto_prune_interval_sec",
        env_nonempty("RUSTORY_AUTO_PRUNE_INTERVAL_SEC"),
        cfg.auto_prune_interval_sec,
        DEFAULT_AUTO_PRUNE_INTERVAL_SEC,
        1,
    )
}

fn resolve_auto_prune_keep_recent(cfg: &config::FileConfig) -> Result<usize> {
    resolve_usize_setting(
        "RUSTORY_AUTO_PRUNE_KEEP_RECENT",
        "auto_prune_keep_recent",
        env_nonempty("RUSTORY_AUTO_PRUNE_KEEP_RECENT"),
        cfg.auto_prune_keep_recent,
        DEFAULT_AUTO_PRUNE_KEEP_RECENT,
        0,
    )
}

fn resolve_auto_prune_marker_path(cfg: &config::FileConfig) -> String {
    resolve_string_setting(
        env_nonempty("RUSTORY_AUTO_PRUNE_MARKER_PATH"),
        cfg.auto_prune_marker_path.clone(),
        DEFAULT_AUTO_PRUNE_MARKER_PATH,
    )
}

fn resolve_auto_tombstone_gc_enabled(cfg: &config::FileConfig) -> Result<bool> {
    resolve_bool_setting(
        "RUSTORY_AUTO_TOMBSTONE_GC",
        env_nonempty("RUSTORY_AUTO_TOMBSTONE_GC"),
        cfg.auto_tombstone_gc,
        false,
    )
}

fn resolve_auto_tombstone_gc_days(cfg: &config::FileConfig) -> Result<u64> {
    resolve_u64_setting(
        "RUSTORY_AUTO_TOMBSTONE_GC_DAYS",
        "auto_tombstone_gc_days",
        env_nonempty("RUSTORY_AUTO_TOMBSTONE_GC_DAYS"),
        cfg.auto_tombstone_gc_days,
        DEFAULT_AUTO_TOMBSTONE_GC_DAYS,
        1,
    )
}

fn resolve_auto_tombstone_gc_interval_sec(cfg: &config::FileConfig) -> Result<u64> {
    resolve_u64_setting(
        "RUSTORY_AUTO_TOMBSTONE_GC_INTERVAL_SEC",
        "auto_tombstone_gc_interval_sec",
        env_nonempty("RUSTORY_AUTO_TOMBSTONE_GC_INTERVAL_SEC"),
        cfg.auto_tombstone_gc_interval_sec,
        DEFAULT_AUTO_TOMBSTONE_GC_INTERVAL_SEC,
        1,
    )
}

fn resolve_auto_tombstone_gc_marker_path(cfg: &config::FileConfig) -> String {
    resolve_string_setting(
        env_nonempty("RUSTORY_AUTO_TOMBSTONE_GC_MARKER_PATH"),
        cfg.auto_tombstone_gc_marker_path.clone(),
        DEFAULT_AUTO_TOMBSTONE_GC_MARKER_PATH,
    )
}

fn env_nonempty(key: &str) -> Option<String> {
    normalize_opt_string(std::env::var(key).ok())
}

fn normalize_opt_string(value: Option<String>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
