use crate::{config, hook, self_update, storage, tracker};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const DAEMON_AUTOSTART_START: &str = "# >>> rustory daemon autostart >>>";
const DAEMON_AUTOSTART_END: &str = "# <<< rustory daemon autostart <<<";
const MANAGED_RC_STATE_FILE: &str = "managed-rc-files.json";
const MANAGED_RC_LOCK_FILE: &str = ".managed-rc-files.lock";
const MANAGED_STATE_HOME_FILE: &str = "managed-state-home";
const MANAGED_STATE_HOMES_FILE: &str = "managed-state-homes.json";
const MANAGED_STATE_HOMES_LOCK_FILE: &str = ".managed-state-homes.lock";
const MAX_MANAGED_RC_STATE_BYTES: u64 = 64 * 1024;
const MAX_MANAGED_RC_FILES: usize = 32;
const MAX_MANAGED_STATE_HOMES: usize = 32;

#[derive(Deserialize)]
struct ManagedRcState {
    version: u32,
    paths: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct ManagedStateHomeHistory {
    version: u32,
    paths: Vec<PathBuf>,
}

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
    pub config_key_paths: Vec<PathBuf>,
    pub state_marker_paths: Vec<PathBuf>,
    pub extra_rc_files: Vec<PathBuf>,
    pub trackers: Vec<String>,
    pub tracker_token: Option<String>,
    pub require_device_membership: bool,
    pub local_peer_id: Option<String>,
    pub local_identity: Option<crate::libp2p::identity::Keypair>,
    pub config_load_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinnedUninstallPaths {
    pub state_files: Vec<PathBuf>,
    pub state_dirs: Vec<PathBuf>,
    pub daemon_service_files: Vec<PathBuf>,
    pub shell_rc_files: Vec<PathBuf>,
}

pub fn load_managed_rc_files() -> Result<Vec<PathBuf>> {
    let home = home_dir()?;
    let path = managed_rc_state_path(&home);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect managed rc state: {}", path.display()));
        }
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "managed rc state must be a regular non-symlink file: {}",
        path.display()
    );
    anyhow::ensure!(
        metadata.len() <= MAX_MANAGED_RC_STATE_BYTES,
        "managed rc state is too large: {}",
        path.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        anyhow::ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "managed rc state permissions are too broad: {}",
            path.display()
        );
    }
    let state: ManagedRcState = serde_json::from_slice(
        &std::fs::read(&path)
            .with_context(|| format!("read managed rc state: {}", path.display()))?,
    )
    .with_context(|| format!("parse managed rc state: {}", path.display()))?;
    anyhow::ensure!(state.version == 1, "unsupported managed rc state version");
    anyhow::ensure!(
        state.paths.len() <= MAX_MANAGED_RC_FILES,
        "too many managed rc paths"
    );
    let mut paths = Vec::new();
    for path in state.paths {
        anyhow::ensure!(path.is_absolute(), "managed rc path must be absolute");
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(paths)
}

pub fn load_managed_state_home() -> Result<Option<PathBuf>> {
    let home = home_dir()?;
    load_managed_state_home_for(&home)
}

fn load_managed_state_home_for(home: &Path) -> Result<Option<PathBuf>> {
    let path = managed_state_home_metadata_path(home);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect managed state home: {}", path.display()));
        }
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "managed state home must be a regular non-symlink file: {}",
        path.display()
    );
    anyhow::ensure!(
        metadata.len() <= 4096,
        "managed state home file is too large: {}",
        path.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        anyhow::ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "managed state home permissions are too broad: {}",
            path.display()
        );
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("read managed state home: {}", path.display()))?;
    let value = raw.trim();
    anyhow::ensure!(
        !value.is_empty() && !value.chars().any(char::is_control),
        "managed state home is empty or contains control characters: {}",
        path.display()
    );
    let state_home = PathBuf::from(value);
    anyhow::ensure!(
        state_home.is_absolute(),
        "managed state home must be absolute: {}",
        state_home.display()
    );
    Ok(Some(state_home))
}

fn load_managed_state_home_history_for(home: &Path) -> Result<Vec<PathBuf>> {
    let path = managed_state_homes_metadata_path(home);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect managed state homes: {}", path.display()));
        }
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "managed state homes must be a regular non-symlink file: {}",
        path.display()
    );
    anyhow::ensure!(
        metadata.len() <= MAX_MANAGED_RC_STATE_BYTES,
        "managed state homes file is too large: {}",
        path.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        anyhow::ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "managed state homes permissions are too broad: {}",
            path.display()
        );
    }
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read managed state homes: {}", path.display()))?;
    let history: ManagedStateHomeHistory =
        serde_json::from_slice(&bytes).context("parse managed state homes json")?;
    anyhow::ensure!(
        history.version == 1,
        "unsupported managed state homes version"
    );
    anyhow::ensure!(
        history.paths.len() <= MAX_MANAGED_STATE_HOMES,
        "too many managed state homes"
    );
    let mut paths = Vec::new();
    for state_home in history.paths {
        let value = state_home
            .to_str()
            .context("managed state home is not valid UTF-8")?;
        anyhow::ensure!(
            state_home.is_absolute() && value.len() <= 4095 && !value.chars().any(char::is_control),
            "managed state home must be an absolute safe path: {}",
            state_home.display()
        );
        if !paths.contains(&state_home) {
            paths.push(state_home);
        }
    }
    Ok(paths)
}

pub fn run_uninstall(request: UninstallRequest) -> Result<()> {
    let pinned_paths =
        resolve_pinned_uninstall_paths(&request.state_marker_paths, &request.extra_rc_files)?;
    run_uninstall_with_pinned_paths(request, &pinned_paths)
}

pub fn run_uninstall_with_pinned_paths(
    request: UninstallRequest,
    pinned_paths: &PinnedUninstallPaths,
) -> Result<()> {
    if request.dry_run || !request.apply {
        print_uninstall_plan_with_paths(&request, pinned_paths)?;
        return Ok(());
    }
    if let Some(error) = request.config_load_error.as_deref() {
        anyhow::bail!(
            "refusing destructive uninstall because config could not be loaded: {error}; fix or move config.toml, then retry"
        );
    }
    validate_path_boundaries_with_paths(&request, pinned_paths)?;

    if request.require_device_membership {
        unregister_from_trackers(&request)?;
        self_update::stop_managed_daemon(&request.install_path)?;
    } else {
        self_update::stop_managed_daemon(&request.install_path)?;
        unregister_from_trackers(&request)?;
    }
    remove_shell_hooks(&pinned_paths.shell_rc_files)?;
    remove_daemon_autostart_blocks(&pinned_paths.shell_rc_files)?;
    remove_daemon_service_files(&pinned_paths.daemon_service_files)?;

    if request.keep_db {
        println!("db=keep path={}", request.db_path.display());
    } else {
        remove_db_path_family(&request.db_path)?;
    }

    if request.keep_state {
        println!("state=keep");
    } else {
        remove_state_locations(ManagedStateLocations {
            files: pinned_paths.state_files.clone(),
            dirs: pinned_paths.state_dirs.clone(),
        })?;
    }

    if request.keep_config {
        println!("config=keep path={}", request.config_path.display());
    } else {
        remove_config_paths(&request.config_path, &request.config_key_paths)?;
    }

    if request.remove_binary {
        remove_file_if_exists(&request.install_path, "binary")?;
    } else {
        println!("binary=keep path={}", request.install_path.display());
    }

    println!("rustory uninstall ok");
    Ok(())
}

pub fn validate_uninstall_request(request: &UninstallRequest) -> Result<()> {
    let pinned_paths =
        resolve_pinned_uninstall_paths(&request.state_marker_paths, &request.extra_rc_files)?;
    validate_path_boundaries_with_paths(request, &pinned_paths)
}

fn print_uninstall_plan_with_paths(
    request: &UninstallRequest,
    pinned_paths: &PinnedUninstallPaths,
) -> Result<()> {
    validate_path_boundaries_with_paths(request, pinned_paths)?;
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
    if let Some(error) = request.config_load_error.as_deref() {
        println!("warn: config load failed; apply is blocked detail={error}");
    }
    println!("hook=planned remove_managed_blocks=true");
    println!("daemon=planned stop_managed_daemon=true");
    for path in &pinned_paths.shell_rc_files {
        println!("rc_file=planned path={}", path.display());
    }
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
    for path in unique_paths(&request.config_key_paths) {
        println!(
            "config_key=planned keep={} path={}",
            request.keep_config,
            path.display()
        );
    }
    println!("state=planned keep={}", request.keep_state);
    for path in &pinned_paths.state_files {
        println!(
            "state_file=planned keep={} path={}",
            request.keep_state,
            path.display()
        );
    }
    for path in &pinned_paths.state_dirs {
        println!(
            "state_dir=planned keep={} path={}",
            request.keep_state,
            path.display()
        );
    }
    println!(
        "binary=planned remove={} path={}",
        request.remove_binary,
        request.install_path.display()
    );
    println!("pass --yes to apply");
    Ok(())
}

fn unregister_from_trackers(request: &UninstallRequest) -> Result<()> {
    let Some(peer_id) = request.local_peer_id.as_deref() else {
        anyhow::ensure!(
            !request.require_device_membership,
            "strict device membership uninstall requires the local PeerId and identity key; no local files were removed"
        );
        println!("tracker_unregister=skipped reason=missing_peer_id");
        return Ok(());
    };
    if request.trackers.is_empty() {
        anyhow::ensure!(
            !request.require_device_membership,
            "strict device membership uninstall requires its authoritative tracker; no local files were removed"
        );
        println!("tracker_unregister=skipped reason=no_trackers");
        return Ok(());
    }
    if request.require_device_membership {
        anyhow::ensure!(
            request.trackers.len() == 1,
            "strict device membership uninstall requires exactly one authoritative tracker; no local files were removed"
        );
        crate::device_retirement::validate_retirement_tracker_url(&request.trackers[0])
            .context("strict device membership uninstall requires an HTTPS tracker")?;
        anyhow::ensure!(
            request.local_identity.is_some(),
            "strict device membership uninstall requires the local identity key; no local files were removed"
        );
    }

    for base_url in &request.trackers {
        let client =
            tracker::TrackerClient::new(base_url.to_string(), request.tracker_token.clone());
        let unregister = match request.local_identity.as_ref() {
            Some(identity) => match tracker::UnregisterRequest::signed(identity) {
                Ok(unregister) => unregister,
                Err(error) => {
                    if request.require_device_membership {
                        return Err(error).context(
                            "sign strict tracker unregister proof; no local files were removed",
                        );
                    }
                    println!("warn: tracker unregister proof failed detail={error:#}");
                    continue;
                }
            },
            None => tracker::UnregisterRequest {
                peer_id: peer_id.to_string(),
                device_proof: None,
            },
        };
        match client.unregister(&unregister) {
            Ok(resp) => {
                if !resp.ok {
                    anyhow::ensure!(
                        !request.require_device_membership,
                        "strict tracker unregister returned ok=false from {base_url}; no local files were removed"
                    );
                    println!("warn: tracker unregister returned ok=false tracker={base_url}");
                    continue;
                }
                println!(
                    "tracker_unregister=ok tracker={} removed={}",
                    base_url, resp.removed
                );
            }
            Err(err) => {
                if request.require_device_membership {
                    return Err(err).with_context(|| {
                        format!(
                            "strict tracker unregister was not confirmed by {base_url}; no local files were removed"
                        )
                    });
                }
                println!("warn: tracker unregister failed tracker={base_url} detail={err:#}")
            }
        }
    }
    Ok(())
}

fn remove_shell_hooks(rc_files: &[PathBuf]) -> Result<()> {
    let reports = hook::remove_managed_hook_blocks_from_paths(rc_files)?;
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

fn remove_daemon_autostart_blocks(rc_files: &[PathBuf]) -> Result<()> {
    let mut removed_files = 0usize;
    for rc_file in rc_files {
        let existing = match std::fs::read_to_string(rc_file) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("read rc file: {}", rc_file.display()));
            }
        };
        let (cleaned, removed_blocks) =
            strip_marker_blocks(&existing, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END)?;
        if removed_blocks == 0 || cleaned == existing {
            continue;
        }
        hook::atomic_write_text_preserving_symlink(rc_file, &cleaned)
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

fn shell_profile_candidates(extra_rc_files: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let home = home_dir()?;
    let mut candidates = [
        ".zshrc",
        ".zprofile",
        ".bashrc",
        ".bash_profile",
        ".profile",
    ]
    .into_iter()
    .map(|name| home.join(name))
    .collect::<Vec<_>>();
    for path in extra_rc_files {
        if !candidates.contains(path) {
            candidates.push(path.clone());
        }
    }
    Ok(candidates)
}

fn strip_marker_blocks(
    content: &str,
    start_marker: &str,
    end_marker: &str,
) -> Result<(String, usize)> {
    hook::strip_managed_marker_blocks(content, &[(start_marker, end_marker)])
}

fn remove_daemon_service_files(paths: &[PathBuf]) -> Result<()> {
    for path in paths {
        remove_file_if_exists(path, "daemon_service")?;
    }
    Ok(())
}

fn daemon_service_files() -> Result<Vec<PathBuf>> {
    let home = home_dir()?;
    Ok(daemon_service_files_for_home(&home))
}

fn daemon_service_files_for_home(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("Library/LaunchAgents/com.rustory.daemon.plist"),
        home.join(".config/systemd/user/rustory.service"),
        home.join(".config/systemd/user/rustory-daemon.service"),
    ]
}

fn remove_db_path_family(db_path: &Path) -> Result<()> {
    if is_in_memory_db_path(db_path) {
        println!("db=remove_skipped path=:memory: reason=sqlite_in_memory");
        return Ok(());
    }
    let default_db = config::expand_home_path(storage::DEFAULT_DB_PATH)?;
    remove_db_path_family_with_default(db_path, &default_db)
}

fn is_in_memory_db_path(path: &Path) -> bool {
    path == Path::new(":memory:")
}

fn remove_db_path_family_with_default(db_path: &Path, default_db: &Path) -> Result<()> {
    remove_file_if_exists(db_path, "db")?;
    remove_file_if_exists(&sidecar_path(db_path, "wal"), "db_wal")?;
    remove_file_if_exists(&sidecar_path(db_path, "shm"), "db_shm")?;
    if same_path(db_path, default_db) {
        let dir = default_db
            .parent()
            .with_context(|| format!("default db path has no parent: {}", default_db.display()))?;
        remove_dir_if_empty(dir, "db_dir")?;
    }
    Ok(())
}

fn remove_config_paths(config_path: &Path, key_paths: &[PathBuf]) -> Result<()> {
    let default_config = config::expand_home_path(config::DEFAULT_CONFIG_PATH)?;
    remove_config_paths_with_default(config_path, key_paths, &default_config)
}

fn remove_config_paths_with_default(
    config_path: &Path,
    key_paths: &[PathBuf],
    default_config: &Path,
) -> Result<()> {
    for path in unique_paths(key_paths) {
        if !same_path(&path, config_path) {
            remove_file_if_exists(&path, "config_key")?;
        }
    }
    remove_file_if_exists(config_path, "config")?;
    if same_path(config_path, default_config) {
        let dir = default_config.parent().with_context(|| {
            format!(
                "default config path has no parent: {}",
                default_config.display()
            )
        })?;
        remove_dir_if_empty(dir, "config_dir")?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedStateLocations {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

fn managed_state_locations(marker_paths: &[PathBuf]) -> Result<ManagedStateLocations> {
    let home = home_dir()?;
    let xdg_state_home = std::env::var_os("XDG_STATE_HOME")
        .and_then(|value| (!value.is_empty()).then_some(PathBuf::from(value)));
    if let Some(path) = xdg_state_home.as_deref()
        && !path.is_absolute()
    {
        anyhow::bail!(
            "XDG_STATE_HOME must be absolute before uninstall: {}",
            path.display()
        );
    }
    let mut state_homes = Vec::new();
    if let Some(path) = xdg_state_home {
        state_homes.push(path);
    }
    if let Some(path) = load_managed_state_home()?
        && !state_homes.contains(&path)
    {
        state_homes.push(path);
    }
    for path in load_managed_state_home_history_for(&home)? {
        if !state_homes.contains(&path) {
            state_homes.push(path);
        }
    }
    Ok(managed_state_locations_for(
        &home,
        &state_homes,
        marker_paths,
    ))
}

pub fn resolve_pinned_uninstall_paths(
    marker_paths: &[PathBuf],
    extra_rc_files: &[PathBuf],
) -> Result<PinnedUninstallPaths> {
    let state = managed_state_locations(marker_paths)?;
    Ok(PinnedUninstallPaths {
        state_files: state.files,
        state_dirs: state.dirs,
        daemon_service_files: daemon_service_files()?,
        shell_rc_files: shell_profile_candidates(extra_rc_files)?,
    })
}

fn managed_state_locations_for(
    home: &Path,
    state_homes: &[PathBuf],
    marker_paths: &[PathBuf],
) -> ManagedStateLocations {
    let mut files = marker_paths.to_vec();
    files.extend([
        home.join("Library/Logs/rustory-daemon.out.log"),
        home.join("Library/Logs/rustory-daemon.err.log"),
        managed_rc_state_path(home),
        managed_rc_lock_path(home),
        managed_state_home_metadata_path(home),
        managed_state_homes_metadata_path(home),
        managed_state_homes_lock_path(home),
    ]);
    let mut dirs = vec![
        home.join(".local/state/rustory"),
        home.join("Library/Logs/rustory"),
    ];
    for base in state_homes {
        dirs.push(base.join("rustory"));
    }
    for dir in &dirs {
        files.push(dir.join("daemon.pid"));
        files.push(dir.join("daemon.log"));
    }
    ManagedStateLocations {
        files: unique_paths(&files),
        dirs: unique_paths(&dirs),
    }
}

fn managed_rc_state_path(home: &Path) -> PathBuf {
    home.join(".config/rustory").join(MANAGED_RC_STATE_FILE)
}

fn managed_rc_lock_path(home: &Path) -> PathBuf {
    home.join(".config/rustory").join(MANAGED_RC_LOCK_FILE)
}

fn managed_state_home_metadata_path(home: &Path) -> PathBuf {
    home.join(".config/rustory").join(MANAGED_STATE_HOME_FILE)
}

fn managed_state_homes_metadata_path(home: &Path) -> PathBuf {
    home.join(".config/rustory").join(MANAGED_STATE_HOMES_FILE)
}

fn managed_state_homes_lock_path(home: &Path) -> PathBuf {
    home.join(".config/rustory")
        .join(MANAGED_STATE_HOMES_LOCK_FILE)
}

fn remove_state_locations(locations: ManagedStateLocations) -> Result<()> {
    for path in locations.files {
        remove_file_if_exists(&path, "state_file")?;
    }
    for path in locations.dirs {
        remove_dir_if_empty(&path, "state_dir")?;
    }
    Ok(())
}

#[cfg(test)]
fn validate_path_boundaries(request: &UninstallRequest) -> Result<()> {
    let pinned_paths =
        resolve_pinned_uninstall_paths(&request.state_marker_paths, &request.extra_rc_files)?;
    validate_path_boundaries_with_paths(request, &pinned_paths)
}

fn validate_path_boundaries_with_paths(
    request: &UninstallRequest,
    pinned_paths: &PinnedUninstallPaths,
) -> Result<()> {
    let mut protected = Vec::new();
    let mut removed = Vec::new();

    if is_in_memory_db_path(&request.db_path) {
        // SQLite's in-memory sentinel is not a filesystem path and has no WAL/SHM sidecars.
    } else if request.keep_db {
        protected.push(("db", request.db_path.clone()));
        protected.push(("db_wal", sidecar_path(&request.db_path, "wal")));
        protected.push(("db_shm", sidecar_path(&request.db_path, "shm")));
    } else {
        removed.push(("db", request.db_path.clone()));
        removed.push(("db_wal", sidecar_path(&request.db_path, "wal")));
        removed.push(("db_shm", sidecar_path(&request.db_path, "shm")));
    }

    let mut config_paths = request.config_key_paths.clone();
    config_paths.push(request.config_path.clone());
    for path in unique_paths(&config_paths) {
        if request.keep_config {
            protected.push(("config", path));
        } else {
            removed.push(("config", path));
        }
    }

    for path in &pinned_paths.state_files {
        if request.keep_state {
            protected.push(("state", path.clone()));
        } else {
            removed.push(("state", path.clone()));
        }
    }
    for path in &pinned_paths.state_dirs {
        if request.keep_state {
            protected.push(("state_dir", path.clone()));
        } else {
            removed.push(("state_dir", path.clone()));
        }
    }

    if request.remove_binary {
        removed.push(("binary", request.install_path.clone()));
    } else {
        protected.push(("binary", request.install_path.clone()));
    }
    for path in &pinned_paths.daemon_service_files {
        removed.push(("daemon_service", path.clone()));
    }
    for path in &pinned_paths.shell_rc_files {
        removed.push(("shell_rc", path.clone()));
    }

    for marker in &request.state_marker_paths {
        anyhow::ensure!(
            pinned_paths.state_files.contains(marker),
            "pinned uninstall paths omit state marker: {}",
            marker.display()
        );
    }
    for rc_file in &request.extra_rc_files {
        anyhow::ensure!(
            pinned_paths.shell_rc_files.contains(rc_file),
            "pinned uninstall paths omit managed rc file: {}",
            rc_file.display()
        );
    }

    validate_uninstall_filesystem_path("binary", &request.install_path)?;
    for (label, path) in &removed {
        validate_uninstall_filesystem_path(label, path)?;
    }

    for (removed_label, removed_path) in &removed {
        if let Some((protected_label, _)) = protected
            .iter()
            .find(|(_, protected_path)| same_path(removed_path, protected_path))
        {
            anyhow::bail!(
                "uninstall path conflict: {removed_label} would remove {} but --keep protects it as {protected_label}; change the path or keep/remove flags",
                removed_path.display()
            );
        }
    }

    Ok(())
}

fn validate_uninstall_filesystem_path(label: &str, path: &Path) -> Result<()> {
    anyhow::ensure!(
        path.is_absolute(),
        "uninstall {label} path must be absolute: {}",
        path.display()
    );
    anyhow::ensure!(
        !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        }),
        "uninstall {label} path must not contain . or .. components: {}",
        path.display()
    );
    Ok(())
}

fn remove_file_if_exists(path: &Path, label: &str) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {
            sync_parent_directory(path)?;
            println!("{label}=removed path={}", path.display());
        }
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

fn remove_dir_if_empty(path: &Path, label: &str) -> Result<()> {
    let mut entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "{label}=remove_skipped path={} reason=missing",
                path.display()
            );
            return Ok(());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("read dir: {}", path.display()));
        }
    };
    if entries.next().is_some() {
        println!("{label}=keep path={} reason=not_empty", path.display());
        return Ok(());
    }
    std::fs::remove_dir(path).with_context(|| format!("remove dir: {}", path.display()))?;
    sync_parent_directory(path)?;
    println!("{label}=removed path={}", path.display());
    Ok(())
}

fn sync_parent_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::File::open(parent)
            .with_context(|| format!("open parent directory for sync: {}", parent.display()))?
            .sync_all()
            .with_context(|| format!("sync parent directory: {}", parent.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_owned();
    value.push(format!("-{suffix}"));
    PathBuf::from(value)
}

fn unique_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut unique = Vec::new();
    for path in paths {
        if !unique.iter().any(|existing| existing == path) {
            unique.push(path.clone());
        }
    }
    unique
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
            strip_marker_blocks(&content, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END).unwrap();

        assert_eq!(removed, 2);
        assert!(!cleaned.contains(DAEMON_AUTOSTART_START));
        assert!(cleaned.contains("keep=1"));
        assert!(cleaned.contains("keep=2"));
    }

    #[test]
    fn strip_marker_blocks_preserves_unmanaged_layout() {
        let prefix = "export KEEP=1\n\n\n";
        let managed = [
            DAEMON_AUTOSTART_START,
            "\nrr daemon &\n",
            DAEMON_AUTOSTART_END,
            "\n",
        ]
        .join("");
        let suffix = "\n\nexport KEEP_TOO=1\n\n\n";
        let content = format!("{prefix}{managed}{suffix}");

        let (cleaned, removed_blocks) =
            strip_marker_blocks(&content, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END).unwrap();

        assert_eq!(removed_blocks, 1);
        assert_eq!(cleaned, format!("{prefix}{suffix}"));
    }

    #[test]
    fn strip_marker_blocks_ignores_quoted_marker_text() {
        let content =
            format!("echo '{DAEMON_AUTOSTART_START}'\nkeep=1\necho '{DAEMON_AUTOSTART_END}'\n");

        let (cleaned, removed_blocks) =
            strip_marker_blocks(&content, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END).unwrap();

        assert_eq!(removed_blocks, 0);
        assert_eq!(cleaned, content);
    }

    #[test]
    fn strip_marker_blocks_rejects_unmatched_marker() {
        let content = format!("{DAEMON_AUTOSTART_START}\nexport KEEP=1\n");

        assert!(
            strip_marker_blocks(&content, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END).is_err()
        );
    }

    #[test]
    fn shell_profile_candidates_include_custom_rc_file() {
        let custom = PathBuf::from("/tmp/rustory-custom-shell.rc");

        let candidates = shell_profile_candidates(std::slice::from_ref(&custom)).unwrap();

        assert!(candidates.contains(&custom));
    }

    #[test]
    fn strict_uninstall_requires_confirmable_signed_tracker_departure() {
        let request = UninstallRequest {
            apply: true,
            dry_run: false,
            keep_db: false,
            keep_config: false,
            keep_state: false,
            remove_binary: true,
            install_path: PathBuf::from("/tmp/rr"),
            config_path: PathBuf::from("/tmp/config.toml"),
            db_path: PathBuf::from("/tmp/history.db"),
            config_key_paths: Vec::new(),
            state_marker_paths: Vec::new(),
            extra_rc_files: Vec::new(),
            trackers: vec!["http://127.0.0.1:8850".to_string()],
            tracker_token: None,
            require_device_membership: true,
            local_peer_id: Some(crate::libp2p::PeerId::random().to_string()),
            local_identity: None,
            config_load_error: None,
        };

        let error = unregister_from_trackers(&request).unwrap_err();
        assert!(format!("{error:#}").contains("requires the local identity key"));
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

    #[test]
    fn in_memory_database_sentinel_never_becomes_a_cleanup_path() {
        assert!(is_in_memory_db_path(Path::new(":memory:")));
        remove_db_path_family(Path::new(":memory:")).unwrap();
        assert!(!is_in_memory_db_path(Path::new("/tmp/:memory:")));
    }

    #[test]
    fn uninstall_rejects_relative_and_parent_traversal_paths() {
        assert!(validate_uninstall_filesystem_path("db", Path::new("history.db")).is_err());
        assert!(
            validate_uninstall_filesystem_path("config", Path::new("/tmp/../etc/passwd")).is_err()
        );
        validate_uninstall_filesystem_path("db", Path::new("/tmp/history.db")).unwrap();
    }

    #[test]
    fn uninstall_allows_relative_paths_only_when_they_are_kept() {
        let request = UninstallRequest {
            apply: true,
            dry_run: false,
            keep_db: true,
            keep_config: true,
            keep_state: true,
            remove_binary: false,
            install_path: PathBuf::from("/tmp/rr"),
            config_path: PathBuf::from("config.toml"),
            db_path: PathBuf::from("history.db"),
            config_key_paths: vec![PathBuf::from("identity.key")],
            state_marker_paths: vec![PathBuf::from("marker")],
            extra_rc_files: Vec::new(),
            trackers: Vec::new(),
            tracker_token: None,
            require_device_membership: false,
            local_peer_id: None,
            local_identity: None,
            config_load_error: None,
        };

        validate_path_boundaries(&request).unwrap();
    }

    #[test]
    fn default_db_cleanup_preserves_unmanaged_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".rustory");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db = db_dir.join("history.db");
        let wal = sidecar_path(&db, "wal");
        let shm = sidecar_path(&db, "shm");
        let backup = db_dir.join("history.backup");
        for path in [&db, &wal, &shm, &backup] {
            std::fs::write(path, b"data").unwrap();
        }

        remove_db_path_family_with_default(&db, &db).unwrap();

        assert!(!db.exists());
        assert!(!wal.exists());
        assert!(!shm.exists());
        assert!(backup.exists());
        assert!(db_dir.exists());
    }

    #[test]
    fn config_cleanup_removes_custom_keys_and_preserves_unmanaged_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config/rustory");
        let custom_dir = dir.path().join("shared");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&custom_dir).unwrap();
        let config_path = config_dir.join("config.toml");
        let default_key = config_dir.join("identity.key");
        let custom_key = custom_dir.join("swarm.key");
        let unmanaged = config_dir.join("notes.txt");
        for path in [&config_path, &default_key, &custom_key, &unmanaged] {
            std::fs::write(path, b"data").unwrap();
        }

        remove_config_paths_with_default(
            &config_path,
            &[default_key.clone(), custom_key.clone()],
            &config_path,
        )
        .unwrap();

        assert!(!config_path.exists());
        assert!(!default_key.exists());
        assert!(!custom_key.exists());
        assert!(unmanaged.exists());
        assert!(config_dir.exists());
    }

    #[test]
    fn state_cleanup_covers_xdg_and_installer_logs_without_removing_unknown_files() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let xdg = dir.path().join("xdg-state");
        let marker = home.join(".config/rustory/async-upload.last");
        let locations = managed_state_locations_for(
            &home,
            std::slice::from_ref(&xdg),
            std::slice::from_ref(&marker),
        );
        for path in &locations.files {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"state").unwrap();
        }
        let unknown = xdg.join("rustory/keep-me.txt");
        std::fs::write(&unknown, b"user").unwrap();

        remove_state_locations(locations).unwrap();

        assert!(!marker.exists());
        assert!(!home.join("Library/Logs/rustory-daemon.out.log").exists());
        assert!(!home.join("Library/Logs/rustory-daemon.err.log").exists());
        assert!(
            !home
                .join(".config/rustory")
                .join(MANAGED_RC_LOCK_FILE)
                .exists()
        );
        assert!(!xdg.join("rustory/daemon.pid").exists());
        assert!(!xdg.join("rustory/daemon.log").exists());
        assert!(unknown.exists());
        assert!(xdg.join("rustory").exists());
    }

    #[test]
    fn managed_state_home_metadata_recovers_custom_installer_state() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let config_dir = home.join(".config/rustory");
        std::fs::create_dir_all(&config_dir).unwrap();
        let state_home = dir.path().join("custom-state");
        let previous_state_home = dir.path().join("previous-state");
        let metadata_path = config_dir.join(MANAGED_STATE_HOME_FILE);
        std::fs::write(&metadata_path, format!("{}\n", state_home.display())).unwrap();
        let history_path = config_dir.join(MANAGED_STATE_HOMES_FILE);
        std::fs::write(
            &history_path,
            serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "paths": [
                    previous_state_home.display().to_string(),
                    state_home.display().to_string()
                ],
            }))
            .unwrap(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&metadata_path, std::fs::Permissions::from_mode(0o600))
                .unwrap();
            std::fs::set_permissions(&history_path, std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }

        assert_eq!(
            load_managed_state_home_for(&home).unwrap(),
            Some(state_home.clone())
        );
        assert_eq!(
            load_managed_state_home_history_for(&home).unwrap(),
            vec![previous_state_home, state_home]
        );
    }

    #[test]
    fn daemon_service_cleanup_includes_legacy_systemd_unit() {
        let home = Path::new("/home/user");
        let paths = daemon_service_files_for_home(home);

        assert!(paths.contains(&home.join(".config/systemd/user/rustory.service")));
        assert!(paths.contains(&home.join(".config/systemd/user/rustory-daemon.service")));
    }

    #[test]
    fn conflicting_keep_and_remove_paths_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("shared.data");
        let request = UninstallRequest {
            apply: true,
            dry_run: false,
            keep_db: true,
            keep_config: false,
            keep_state: true,
            remove_binary: false,
            install_path: dir.path().join("rr"),
            config_path: dir.path().join("config.toml"),
            db_path: shared.clone(),
            config_key_paths: vec![shared.clone()],
            state_marker_paths: Vec::new(),
            extra_rc_files: Vec::new(),
            trackers: Vec::new(),
            tracker_token: None,
            require_device_membership: false,
            local_peer_id: None,
            local_identity: None,
            config_load_error: None,
        };

        let err = validate_path_boundaries(&request).unwrap_err();

        assert!(format!("{err:#}").contains("uninstall path conflict"));
        assert!(format!("{err:#}").contains(&shared.display().to_string()));
    }
}
