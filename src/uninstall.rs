use crate::{config, hook, self_update, storage, tracker};
use anyhow::{Context, Result};
use std::ffi::OsString;
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
    pub config_key_paths: Vec<PathBuf>,
    pub state_marker_paths: Vec<PathBuf>,
    pub extra_rc_files: Vec<PathBuf>,
    pub trackers: Vec<String>,
    pub tracker_token: Option<String>,
    pub local_peer_id: Option<String>,
    pub config_load_error: Option<String>,
}

pub fn run_uninstall(request: UninstallRequest) -> Result<()> {
    if request.dry_run || !request.apply {
        print_uninstall_plan(&request)?;
        return Ok(());
    }
    if let Some(error) = request.config_load_error.as_deref() {
        anyhow::bail!(
            "refusing destructive uninstall because config could not be loaded: {error}; fix or move config.toml, then retry"
        );
    }
    validate_path_boundaries(&request)?;

    self_update::stop_managed_daemon(&request.install_path)?;
    unregister_from_trackers(&request);
    remove_shell_hooks(&request.extra_rc_files)?;
    remove_daemon_autostart_blocks(&request.extra_rc_files)?;
    remove_daemon_service_files()?;

    if request.keep_db {
        println!("db=keep path={}", request.db_path.display());
    } else {
        remove_db_path_family(&request.db_path)?;
    }

    if request.keep_state {
        println!("state=keep");
    } else {
        remove_state_paths(&request.state_marker_paths)?;
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

fn print_uninstall_plan(request: &UninstallRequest) -> Result<()> {
    validate_path_boundaries(request)?;
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
    for path in unique_paths(&request.extra_rc_files) {
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
    let locations = managed_state_locations(&request.state_marker_paths)?;
    for path in locations.files {
        println!(
            "state_file=planned keep={} path={}",
            request.keep_state,
            path.display()
        );
    }
    for path in locations.dirs {
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

fn remove_shell_hooks(extra_rc_files: &[PathBuf]) -> Result<()> {
    let reports = hook::remove_existing_managed_hook_blocks(extra_rc_files)?;
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

fn remove_daemon_autostart_blocks(extra_rc_files: &[PathBuf]) -> Result<()> {
    let mut removed_files = 0usize;
    for rc_file in shell_profile_candidates(extra_rc_files)? {
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

fn strip_marker_blocks(content: &str, start_marker: &str, end_marker: &str) -> (String, usize) {
    let mut rest = content;
    let mut output = String::with_capacity(content.len());
    let mut removed = 0usize;
    while let Some(start) = find_line_marker(rest, start_marker) {
        output.push_str(&rest[..start]);
        let after_start_offset = marker_line_end(rest, start, start_marker);
        let after_start = &rest[after_start_offset..];
        let Some(end) = find_line_marker(after_start, end_marker) else {
            output.push_str(&rest[start..]);
            return (output, removed);
        };
        let skip = after_start_offset + marker_line_end(after_start, end, end_marker);
        rest = &rest[skip..];
        removed += 1;
    }
    output.push_str(rest);
    (output, removed)
}

fn find_line_marker(content: &str, marker: &str) -> Option<usize> {
    content.match_indices(marker).find_map(|(offset, _)| {
        let starts_line = offset == 0 || content.as_bytes().get(offset - 1) == Some(&b'\n');
        let after = offset + marker.len();
        let ends_line = after == content.len()
            || content.as_bytes().get(after) == Some(&b'\n')
            || (content.as_bytes().get(after) == Some(&b'\r')
                && content.as_bytes().get(after + 1) == Some(&b'\n'));
        (starts_line && ends_line).then_some(offset)
    })
}

fn marker_line_end(content: &str, offset: usize, marker: &str) -> usize {
    let after = offset + marker.len();
    if content[after..].starts_with("\r\n") {
        after + 2
    } else if content[after..].starts_with('\n') {
        after + 1
    } else {
        after
    }
}

fn remove_daemon_service_files() -> Result<()> {
    for path in daemon_service_files()? {
        remove_file_if_exists(&path, "daemon_service")?;
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
    let default_db = config::expand_home_path(storage::DEFAULT_DB_PATH)?;
    remove_db_path_family_with_default(db_path, &default_db)
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
    Ok(managed_state_locations_for(
        &home,
        xdg_state_home.as_deref(),
        marker_paths,
    ))
}

fn managed_state_locations_for(
    home: &Path,
    xdg_state_home: Option<&Path>,
    marker_paths: &[PathBuf],
) -> ManagedStateLocations {
    let mut files = marker_paths.to_vec();
    files.extend([
        home.join("Library/Logs/rustory-daemon.out.log"),
        home.join("Library/Logs/rustory-daemon.err.log"),
    ]);
    let mut dirs = vec![
        home.join(".local/state/rustory"),
        home.join("Library/Logs/rustory"),
    ];
    if let Some(base) = xdg_state_home {
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

fn remove_state_paths(marker_paths: &[PathBuf]) -> Result<()> {
    let locations = managed_state_locations(marker_paths)?;
    remove_state_locations(locations)
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

fn validate_path_boundaries(request: &UninstallRequest) -> Result<()> {
    let state_locations = managed_state_locations(&request.state_marker_paths)?;
    let mut protected = Vec::new();
    let mut removed = Vec::new();

    if request.keep_db {
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

    for path in state_locations.files {
        if request.keep_state {
            protected.push(("state", path));
        } else {
            removed.push(("state", path));
        }
    }

    if request.remove_binary {
        removed.push(("binary", request.install_path.clone()));
    } else {
        protected.push(("binary", request.install_path.clone()));
    }
    for path in daemon_service_files()? {
        removed.push(("daemon_service", path));
    }
    for path in shell_profile_candidates(&request.extra_rc_files)? {
        removed.push(("shell_rc", path));
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
    println!("{label}=removed path={}", path.display());
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
            strip_marker_blocks(&content, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END);

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
            strip_marker_blocks(&content, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END);

        assert_eq!(removed_blocks, 1);
        assert_eq!(cleaned, format!("{prefix}{suffix}"));
    }

    #[test]
    fn strip_marker_blocks_ignores_quoted_marker_text() {
        let content =
            format!("echo '{DAEMON_AUTOSTART_START}'\nkeep=1\necho '{DAEMON_AUTOSTART_END}'\n");

        let (cleaned, removed_blocks) =
            strip_marker_blocks(&content, DAEMON_AUTOSTART_START, DAEMON_AUTOSTART_END);

        assert_eq!(removed_blocks, 0);
        assert_eq!(cleaned, content);
    }

    #[test]
    fn shell_profile_candidates_include_custom_rc_file() {
        let custom = PathBuf::from("/tmp/rustory-custom-shell.rc");

        let candidates = shell_profile_candidates(std::slice::from_ref(&custom)).unwrap();

        assert!(candidates.contains(&custom));
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
        let locations =
            managed_state_locations_for(&home, Some(&xdg), std::slice::from_ref(&marker));
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
        assert!(!xdg.join("rustory/daemon.pid").exists());
        assert!(!xdg.join("rustory/daemon.log").exists());
        assert!(unknown.exists());
        assert!(xdg.join("rustory").exists());
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
            local_peer_id: None,
            config_load_error: None,
        };

        let err = validate_path_boundaries(&request).unwrap_err();

        assert!(format!("{err:#}").contains("uninstall path conflict"));
        assert!(format!("{err:#}").contains(&shared.display().to_string()));
    }
}
