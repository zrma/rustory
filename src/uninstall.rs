use crate::{config, hook, self_update, storage, tracker};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const DAEMON_AUTOSTART_START: &str = "# >>> rustory daemon autostart >>>";
const DAEMON_AUTOSTART_END: &str = "# <<< rustory daemon autostart <<<";

#[derive(Debug, Clone)]
pub struct UninstallRequest {
    pub apply: bool,
    pub dry_run: bool,
    pub keep_db: bool,
    pub keep_config: bool,
    pub keep_state: bool,
    pub remove_binary: bool,
    pub install_path: PathBuf,
    pub config_path: PathBuf,
    pub db_path: PathBuf,
    pub trackers: Vec<String>,
    pub tracker_token: Option<String>,
    pub local_peer_id: Option<String>,
}

pub fn run_uninstall(request: UninstallRequest) -> Result<()> {
    if request.dry_run || !request.apply {
        print_uninstall_plan(&request);
        return Ok(());
    }

    self_update::stop_managed_daemon(&request.install_path);
    unregister_from_trackers(&request);
    remove_shell_hooks()?;
    remove_daemon_autostart_blocks()?;
    remove_daemon_service_files()?;

    if request.keep_db {
        println!("db=keep path={}", request.db_path.display());
    } else {
        remove_db_path_family(&request.db_path)?;
    }

    if request.keep_config {
        println!("config=keep path={}", request.config_path.display());
    } else {
        remove_config_path(&request.config_path)?;
    }

    if request.keep_state {
        println!("state=keep");
    } else {
        remove_state_dirs()?;
    }

    if request.remove_binary {
        remove_file_if_exists(&request.install_path, "binary")?;
    } else {
        println!("binary=keep path={}", request.install_path.display());
    }

    println!("rustory uninstall ok");
    Ok(())
}

fn print_uninstall_plan(request: &UninstallRequest) {
    println!(
        "uninstall plan: apply=false peer_id={} trackers={} hook=true daemon=true db={} config={} state={} binary={}",
        request.local_peer_id.as_deref().unwrap_or("(missing)"),
        request.trackers.len(),
        !request.keep_db,
        !request.keep_config,
        !request.keep_state,
        request.remove_binary
    );
    for tracker in &request.trackers {
        println!("tracker_unregister=planned tracker={tracker}");
    }
    println!("hook=planned remove_managed_blocks=true");
    println!("daemon=planned stop_managed_daemon=true");
    println!(
        "db=planned keep={} path={}",
        request.keep_db,
        request.db_path.display()
    );
    println!(
        "config=planned keep={} path={}",
        request.keep_config,
        request.config_path.display()
    );
    println!("state=planned keep={}", request.keep_state);
    println!(
        "binary=planned remove={} path={}",
        request.remove_binary,
        request.install_path.display()
    );
    println!("pass --yes to apply");
}

fn unregister_from_trackers(request: &UninstallRequest) {
    let Some(peer_id) = request.local_peer_id.as_deref() else {
        println!("tracker_unregister=skipped reason=missing_peer_id");
        return;
    };
    if request.trackers.is_empty() {
        println!("tracker_unregister=skipped reason=no_trackers");
        return;
    }

    for base_url in &request.trackers {
        let client =
            tracker::TrackerClient::new(base_url.to_string(), request.tracker_token.clone());
        match client.unregister(&tracker::UnregisterRequest {
            peer_id: peer_id.to_string(),
        }) {
            Ok(resp) => println!(
                "tracker_unregister=ok tracker={} removed={}",
                base_url, resp.removed
            ),
            Err(err) => {
                println!("warn: tracker unregister failed tracker={base_url} detail={err:#}")
            }
        }
    }
}

fn remove_shell_hooks() -> Result<()> {
    let reports = hook::remove_existing_managed_hook_blocks()?;
    if reports.is_empty() {
        println!("hook=remove_skipped reason=no_managed_hook_blocks");
        return Ok(());
    }

    for report in reports {
        println!(
            "hook=removed shell={} rc_file={} removed_blocks={} status={:?}",
            report.shell.name(),
            report.rc_file.display(),
            report.removed_blocks,
            report.status
        );
    }
    Ok(())
}

fn remove_daemon_autostart_blocks() -> Result<()> {
    let mut removed_files = 0usize;
    for rc_file in shell_profile_candidates()? {
        let existing = match std::fs::read_to_string(&rc_file) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("read rc file: {}", rc_file.display()));
            }
        };
        let (cleaned, removed_blocks) =
            strip_marker_blocks(&existing, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END);
        if removed_blocks == 0 || cleaned == existing {
            continue;
        }
        std::fs::write(&rc_file, cleaned)
            .with_context(|| format!("write rc file: {}", rc_file.display()))?;
        removed_files += 1;
        println!(
            "daemon_autostart=removed rc_file={} removed_blocks={}",
            rc_file.display(),
            removed_blocks
        );
    }
    if removed_files == 0 {
        println!("daemon_autostart=remove_skipped reason=no_managed_blocks");
    }
    Ok(())
}

fn shell_profile_candidates() -> Result<Vec<PathBuf>> {
    let home = home_dir()?;
    Ok([
        ".zshrc",
        ".zprofile",
        ".bashrc",
        ".bash_profile",
        ".profile",
    ]
    .into_iter()
    .map(|name| home.join(name))
    .collect())
}

fn strip_marker_blocks(content: &str, start_marker: &str, end_marker: &str) -> (String, usize) {
    let mut rest = content;
    let mut output = String::with_capacity(content.len());
    let mut removed = 0usize;
    while let Some(start) = rest.find(start_marker) {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + start_marker.len()..];
        let Some(end) = after_start.find(end_marker) else {
            output.push_str(&rest[start..]);
            return (trim_repeated_blank_lines(&output), removed);
        };
        let mut skip = start + start_marker.len() + end + end_marker.len();
        if rest[skip..].starts_with('\n') {
            skip += 1;
        }
        rest = &rest[skip..];
        removed += 1;
    }
    output.push_str(rest);
    (trim_repeated_blank_lines(&output), removed)
}

fn trim_repeated_blank_lines(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut blank = false;
    for line in content.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && blank {
            continue;
        }
        result.push_str(line);
        result.push('\n');
        blank = is_blank;
    }
    result.trim_matches('\n').to_string()
}

fn remove_daemon_service_files() -> Result<()> {
    let home = home_dir()?;
    remove_file_if_exists(
        &home.join("Library/LaunchAgents/com.rustory.daemon.plist"),
        "daemon_service",
    )?;
    remove_file_if_exists(
        &home.join(".config/systemd/user/rustory.service"),
        "daemon_service",
    )?;
    Ok(())
}

fn remove_db_path_family(db_path: &Path) -> Result<()> {
    let default_db = config::expand_home_path(storage::DEFAULT_DB_PATH)?;
    if same_path(db_path, &default_db) {
        let dir = default_db
            .parent()
            .with_context(|| format!("default db path has no parent: {}", default_db.display()))?;
        remove_dir_if_exists(dir, "db_dir")?;
        return Ok(());
    }

    remove_file_if_exists(db_path, "db")?;
    remove_file_if_exists(&sidecar_path(db_path, "wal"), "db_wal")?;
    remove_file_if_exists(&sidecar_path(db_path, "shm"), "db_shm")?;
    Ok(())
}

fn remove_config_path(config_path: &Path) -> Result<()> {
    let default_config = config::expand_home_path(config::DEFAULT_CONFIG_PATH)?;
    if same_path(config_path, &default_config) {
        let dir = default_config.parent().with_context(|| {
            format!(
                "default config path has no parent: {}",
                default_config.display()
            )
        })?;
        remove_dir_if_exists(dir, "config_dir")?;
        return Ok(());
    }

    remove_file_if_exists(config_path, "config")?;
    Ok(())
}

fn remove_state_dirs() -> Result<()> {
    let home = home_dir()?;
    remove_dir_if_exists(&home.join(".local/state/rustory"), "state_dir")?;
    remove_dir_if_exists(&home.join("Library/Logs/rustory"), "state_dir")?;
    Ok(())
}

fn remove_file_if_exists(path: &Path, label: &str) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => println!("{label}=removed path={}", path.display()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "{label}=remove_skipped path={} reason=missing",
                path.display()
            )
        }
        Err(err) => {
            return Err(err).with_context(|| format!("remove file: {}", path.display()));
        }
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path, label: &str) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => println!("{label}=removed path={}", path.display()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "{label}=remove_skipped path={} reason=missing",
                path.display()
            )
        }
        Err(err) => {
            return Err(err).with_context(|| format!("remove dir: {}", path.display()));
        }
    }
    Ok(())
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}-{suffix}", path.display()))
}

fn same_path(left: &Path, right: &Path) -> bool {
    normalize_path_for_compare(left) == normalize_path_for_compare(right)
}

fn normalize_path_for_compare(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(path) => path,
        Err(_) => path.to_path_buf(),
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME env var not set")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_marker_blocks_removes_multiple_blocks() {
        let content = [
            "keep=1\n",
            DAEMON_AUTOSTART_START,
            "\nrr daemon &\n",
            DAEMON_AUTOSTART_END,
            "\n\n",
            DAEMON_AUTOSTART_START,
            "\nrr daemon &\n",
            DAEMON_AUTOSTART_END,
            "\nkeep=2\n",
        ]
        .join("");

        let (cleaned, removed) =
            strip_marker_blocks(&content, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END);

        assert_eq!(removed, 2);
        assert!(!cleaned.contains(DAEMON_AUTOSTART_START));
        assert!(cleaned.contains("keep=1"));
        assert!(cleaned.contains("keep=2"));
    }

    #[test]
    fn sidecar_path_appends_sqlite_wal_and_shm_suffixes() {
        let db = Path::new("/tmp/rustory/history.db");

        assert_eq!(
            sidecar_path(db, "wal"),
            PathBuf::from("/tmp/rustory/history.db-wal")
        );
        assert_eq!(
            sidecar_path(db, "shm"),
            PathBuf::from("/tmp/rustory/history.db-shm")
        );
    }
}
