use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const MANAGED_DAEMON_LOG_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const MANAGED_DAEMON_LOG_CHECK_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedLogCleanupStatus {
    Cleaned,
    Ok,
    Missing,
    SkippedUnsafe,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedLogCleanup {
    pub path: PathBuf,
    pub status: ManagedLogCleanupStatus,
    pub previous_bytes: Option<u64>,
    pub detail: Option<String>,
}

pub fn managed_daemon_log_paths() -> Result<Vec<PathBuf>> {
    let home = std::env::var_os("HOME").context("HOME env var not set")?;
    let xdg_state_home = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    let mut paths = managed_daemon_log_paths_for(
        current_platform(),
        Path::new(&home),
        xdg_state_home.as_deref(),
    )?;
    let pinned = crate::uninstall::resolve_pinned_uninstall_paths(&[], &[])?;
    extend_with_managed_state_logs(&mut paths, &pinned.state_dirs);
    Ok(paths)
}

pub fn cleanup_managed_daemon_logs() -> Result<Vec<ManagedLogCleanup>> {
    let paths = managed_daemon_log_paths()?;
    Ok(paths
        .iter()
        .map(|path| cleanup_log_file(path, MANAGED_DAEMON_LOG_MAX_BYTES))
        .collect())
}

pub fn cleanup_log_file(path: &Path, max_bytes: u64) -> ManagedLogCleanup {
    let initial = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return cleanup_result(path, ManagedLogCleanupStatus::Missing, None, None);
        }
        Err(err) => {
            return cleanup_result(
                path,
                ManagedLogCleanupStatus::Failed,
                None,
                Some(format!("stat failed: {err}")),
            );
        }
    };

    if let Some(reason) = unsafe_file_reason(&initial) {
        return cleanup_result(
            path,
            ManagedLogCleanupStatus::SkippedUnsafe,
            Some(initial.len()),
            Some(reason),
        );
    }
    if initial.len() <= max_bytes {
        return cleanup_result(path, ManagedLogCleanupStatus::Ok, Some(initial.len()), None);
    }

    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }

    let file = match options.open(path) {
        Ok(file) => file,
        Err(err) => {
            return cleanup_result(
                path,
                ManagedLogCleanupStatus::Failed,
                Some(initial.len()),
                Some(format!("secure open failed: {err}")),
            );
        }
    };
    let opened = match file.metadata() {
        Ok(metadata) => metadata,
        Err(err) => {
            return cleanup_result(
                path,
                ManagedLogCleanupStatus::Failed,
                Some(initial.len()),
                Some(format!("opened-file stat failed: {err}")),
            );
        }
    };
    if let Some(reason) = unsafe_file_reason(&opened) {
        return cleanup_result(
            path,
            ManagedLogCleanupStatus::SkippedUnsafe,
            Some(opened.len()),
            Some(reason),
        );
    }
    if opened.len() <= max_bytes {
        return cleanup_result(path, ManagedLogCleanupStatus::Ok, Some(opened.len()), None);
    }

    match file.set_len(0) {
        Ok(()) => cleanup_result(
            path,
            ManagedLogCleanupStatus::Cleaned,
            Some(opened.len()),
            None,
        ),
        Err(err) => cleanup_result(
            path,
            ManagedLogCleanupStatus::Failed,
            Some(opened.len()),
            Some(format!("truncate failed: {err}")),
        ),
    }
}

pub fn cleanup_managed_daemon_logs_with_warnings() {
    match cleanup_managed_daemon_logs() {
        Ok(results) => {
            for result in results {
                match result.status {
                    ManagedLogCleanupStatus::Cleaned => eprintln!(
                        "daemon logs: cleaned path={} previous_bytes={} max_bytes={}",
                        result.path.display(),
                        result.previous_bytes.unwrap_or(0),
                        MANAGED_DAEMON_LOG_MAX_BYTES
                    ),
                    ManagedLogCleanupStatus::SkippedUnsafe | ManagedLogCleanupStatus::Failed => {
                        eprintln!(
                            "warn: daemon log cleanup skipped path={} detail={}",
                            result.path.display(),
                            result.detail.as_deref().unwrap_or("unknown")
                        );
                    }
                    ManagedLogCleanupStatus::Ok | ManagedLogCleanupStatus::Missing => {}
                }
            }
        }
        Err(err) => eprintln!("warn: resolve managed daemon logs for cleanup: {err:#}"),
    }
}

pub fn spawn_managed_daemon_log_monitor(stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        while !sleep_until_stopped(MANAGED_DAEMON_LOG_CHECK_INTERVAL, stop.as_ref()) {
            cleanup_managed_daemon_logs_with_warnings();
        }
    });
}

fn sleep_until_stopped(duration: Duration, stop: &AtomicBool) -> bool {
    for _ in 0..duration.as_secs() {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    stop.load(Ordering::SeqCst)
}

fn cleanup_result(
    path: &Path,
    status: ManagedLogCleanupStatus,
    previous_bytes: Option<u64>,
    detail: Option<String>,
) -> ManagedLogCleanup {
    ManagedLogCleanup {
        path: path.to_path_buf(),
        status,
        previous_bytes,
        detail,
    }
}

fn unsafe_file_reason(metadata: &fs::Metadata) -> Option<String> {
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Some("path is not a regular file".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Some(format!(
                "path has {} hard links; expected exactly one",
                metadata.nlink()
            ));
        }
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Some(format!(
                "path owner uid={} does not match current uid={}",
                metadata.uid(),
                unsafe { libc::geteuid() }
            ));
        }
    }
    None
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Macos,
    Linux,
    Other,
}

fn current_platform() -> Platform {
    #[cfg(target_os = "macos")]
    return Platform::Macos;
    #[cfg(target_os = "linux")]
    return Platform::Linux;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Platform::Other;
}

fn managed_daemon_log_paths_for(
    platform: Platform,
    home: &Path,
    xdg_state_home: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    match platform {
        Platform::Macos => Ok(vec![
            home.join("Library/Logs/rustory-daemon.out.log"),
            home.join("Library/Logs/rustory-daemon.err.log"),
            home.join("Library/Logs/rustory-retirement.log"),
        ]),
        Platform::Linux => {
            let state_home = match xdg_state_home {
                Some(path) if path.is_absolute() => path.to_path_buf(),
                Some(path) => anyhow::bail!(
                    "XDG_STATE_HOME must be absolute for managed log cleanup: {}",
                    path.display()
                ),
                None => home.join(".local/state"),
            };
            Ok(vec![
                state_home.join("rustory/daemon.log"),
                state_home.join("rustory/retirement.log"),
            ])
        }
        Platform::Other => Ok(Vec::new()),
    }
}

fn extend_with_managed_state_logs(paths: &mut Vec<PathBuf>, state_dirs: &[PathBuf]) {
    for state_dir in state_dirs {
        paths.push(state_dir.join("daemon.log"));
        paths.push(state_dir.join("retirement.log"));
    }
    paths.sort();
    paths.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn oversized_regular_log_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.log");
        fs::write(&path, b"0123456789").unwrap();

        let result = cleanup_log_file(&path, 4);

        assert_eq!(result.status, ManagedLogCleanupStatus::Cleaned);
        assert_eq!(result.previous_bytes, Some(10));
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
    }

    #[test]
    fn existing_append_writer_continues_from_truncated_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.log");
        let mut writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        writer.write_all(b"0123456789").unwrap();
        writer.flush().unwrap();

        let result = cleanup_log_file(&path, 4);
        writer.write_all(b"next").unwrap();
        writer.flush().unwrap();

        assert_eq!(result.status, ManagedLogCleanupStatus::Cleaned);
        assert_eq!(fs::read(path).unwrap(), b"next");
    }

    #[test]
    fn log_within_limit_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.log");
        fs::write(&path, b"keep").unwrap();

        let result = cleanup_log_file(&path, 4);

        assert_eq!(result.status, ManagedLogCleanupStatus::Ok);
        assert_eq!(fs::read(path).unwrap(), b"keep");
    }

    #[test]
    fn missing_log_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.log");

        let result = cleanup_log_file(&path, 4);

        assert_eq!(result.status, ManagedLogCleanupStatus::Missing);
        assert!(!path.exists());
    }

    #[test]
    fn managed_state_history_adds_daemon_and_retirement_logs_once() {
        let mut paths = vec![PathBuf::from("/state/a/rustory/daemon.log")];
        extend_with_managed_state_logs(
            &mut paths,
            &[
                PathBuf::from("/state/a/rustory"),
                PathBuf::from("/state/b/rustory"),
            ],
        );

        assert_eq!(
            paths,
            vec![
                PathBuf::from("/state/a/rustory/daemon.log"),
                PathBuf::from("/state/a/rustory/retirement.log"),
                PathBuf::from("/state/b/rustory/daemon.log"),
                PathBuf::from("/state/b/rustory/retirement.log"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_is_rejected_without_touching_target() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("daemon.log");
        fs::write(&target, b"do not truncate").unwrap();
        symlink(&target, &link).unwrap();

        let result = cleanup_log_file(&link, 4);

        assert_eq!(result.status, ManagedLogCleanupStatus::SkippedUnsafe);
        assert_eq!(fs::read(target).unwrap(), b"do not truncate");
    }

    #[cfg(unix)]
    #[test]
    fn hard_link_is_rejected_without_touching_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("daemon.log");
        fs::write(&target, b"do not truncate").unwrap();
        fs::hard_link(&target, &link).unwrap();

        let result = cleanup_log_file(&link, 4);

        assert_eq!(result.status, ManagedLogCleanupStatus::SkippedUnsafe);
        assert_eq!(fs::read(target).unwrap(), b"do not truncate");
    }

    #[test]
    fn directory_is_rejected() {
        let dir = tempfile::tempdir().unwrap();

        let result = cleanup_log_file(dir.path(), 0);

        assert_eq!(result.status, ManagedLogCleanupStatus::SkippedUnsafe);
    }

    #[test]
    fn platform_paths_cover_only_rustory_owned_logs() {
        let home = Path::new("/home/tester");
        assert_eq!(
            managed_daemon_log_paths_for(Platform::Macos, home, None).unwrap(),
            vec![
                home.join("Library/Logs/rustory-daemon.out.log"),
                home.join("Library/Logs/rustory-daemon.err.log"),
                home.join("Library/Logs/rustory-retirement.log")
            ]
        );
        assert_eq!(
            managed_daemon_log_paths_for(Platform::Linux, home, Some(Path::new("/state/tester")))
                .unwrap(),
            vec![
                PathBuf::from("/state/tester/rustory/daemon.log"),
                PathBuf::from("/state/tester/rustory/retirement.log")
            ]
        );
        assert!(
            managed_daemon_log_paths_for(Platform::Linux, home, Some(Path::new("relative/state")))
                .is_err()
        );
        assert!(
            managed_daemon_log_paths_for(Platform::Other, home, None)
                .unwrap()
                .is_empty()
        );
    }
}
