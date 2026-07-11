use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_RELEASE_REPO: &str = "zrma/rustory";

#[cfg(target_os = "linux")]
const SYSTEMD_USER_UNIT: &str = "rustory.service";
#[cfg(target_os = "linux")]
const LEGACY_SYSTEMD_USER_UNITS: &[&str] = &["rustory-daemon.service"];

const MAX_ASSET_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: u64 = 64 * 1024;
const DOWNLOADED_BINARY_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRequest {
    pub version: String,
    pub repo: String,
    pub asset_base_url: Option<String>,
    pub asset_url: Option<String>,
    pub checksum_url: Option<String>,
    pub sha256: Option<String>,
    pub install_path: Option<PathBuf>,
    pub dry_run: bool,
    pub restart_daemon: bool,
    pub allow_insecure_download: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdatePlan {
    version: String,
    repo: String,
    target: String,
    asset_name: String,
    asset_url: String,
    checksum_url: Option<String>,
    install_path: PathBuf,
}

pub fn run_update(request: UpdateRequest) -> Result<()> {
    let plan = build_update_plan(&request)?;

    println!(
        "update plan: current_version={} current_revision={} target={} version={} install_path={}",
        crate::build_info::VERSION,
        crate::build_info::BUILD_REVISION,
        plan.target,
        plan.version,
        plan.install_path.display()
    );
    println!("asset_url: {}", plan.asset_url);
    match (&request.sha256, &plan.checksum_url) {
        (Some(_), _) => println!("checksum: provided via --sha256"),
        (None, Some(url)) => println!("checksum_url: {url}"),
        (None, None) => println!("checksum: unavailable"),
    }

    if request.dry_run {
        println!("dry-run: no download or replacement performed");
        return Ok(());
    }

    let bytes = download_bytes(&plan.asset_url, MAX_ASSET_BYTES)
        .with_context(|| format!("download release asset: {}", plan.asset_url))?;
    let expected = resolve_expected_sha256(&request, &plan)?;
    verify_sha256(&bytes, &expected)?;

    if installed_binary_matches(&plan.install_path, &bytes)? {
        make_executable(&plan.install_path)?;
        verify_downloaded_binary(&plan.install_path, &plan.version)?;
        println!(
            "update: installed binary already matches downloaded asset; no replacement performed"
        );
        auto_fix_managed_hook_blocks(&plan.install_path);
        handle_post_update_daemon(&plan.install_path, false, request.restart_daemon);
        return Ok(());
    }

    install_binary(&bytes, &plan.install_path, &plan.version)?;

    println!("updated rr: {}", plan.install_path.display());
    auto_fix_managed_hook_blocks(&plan.install_path);
    handle_post_update_daemon(&plan.install_path, true, request.restart_daemon);
    Ok(())
}

fn auto_fix_managed_hook_blocks(install_path: &Path) {
    match crate::hook::auto_fix_existing_managed_hook_blocks(install_path) {
        Ok(reports) if reports.is_empty() => {
            println!("hook_auto_fix=skipped reason=no_managed_hook_blocks");
        }
        Ok(reports) => {
            for report in reports {
                let status = match report.status {
                    crate::hook::ManagedHookFixStatus::Fixed => "fixed",
                    crate::hook::ManagedHookFixStatus::Ok => "ok",
                    crate::hook::ManagedHookFixStatus::Skipped => "skipped",
                };
                println!(
                    "hook_auto_fix={status} shell={} rc_file={} removed_blocks={}",
                    report.shell.name(),
                    report.rc_file.display(),
                    report.removed_blocks
                );
            }
        }
        Err(err) => {
            eprintln!("warn: hook auto-fix failed: {err:#}");
        }
    }
}

fn build_update_plan(request: &UpdateRequest) -> Result<UpdatePlan> {
    if request.asset_base_url.is_some() && request.asset_url.is_some() {
        anyhow::bail!("pass only one of --asset-base-url or --asset-url");
    }

    let version = normalize_version(&request.version)?;
    let repo = normalize_repo(&request.repo)?;
    let target = current_release_target()?.to_string();
    let asset_name = release_asset_name(&target);
    let asset_url = match request.asset_url.as_deref().and_then(normalize_nonempty) {
        Some(url) => url,
        None => match request
            .asset_base_url
            .as_deref()
            .and_then(normalize_nonempty)
        {
            Some(base_url) => format!("{}/{}", base_url.trim_end_matches('/'), asset_name),
            None => github_release_asset_url(&repo, &version, &asset_name),
        },
    };
    let asset_name = asset_name_from_url(&asset_url).unwrap_or(asset_name);
    let checksum_url = request
        .checksum_url
        .as_deref()
        .and_then(normalize_nonempty)
        .or_else(|| {
            request
                .sha256
                .is_none()
                .then(|| format!("{asset_url}.sha256"))
        });
    validate_release_download_urls(
        &asset_url,
        checksum_url.as_deref(),
        request.sha256.is_some(),
        request.allow_insecure_download,
    )?;
    let install_path = match request.install_path.clone() {
        Some(path) => path,
        None => std::env::current_exe().context("resolve current rr executable path")?,
    };

    Ok(UpdatePlan {
        version,
        repo,
        target,
        asset_name,
        asset_url,
        checksum_url,
        install_path,
    })
}

fn normalize_version(raw: &str) -> Result<String> {
    normalize_nonempty(raw).context("--version must not be empty")
}

fn normalize_repo(raw: &str) -> Result<String> {
    let repo = normalize_nonempty(raw).context("--repo must not be empty")?;
    if repo.chars().any(char::is_whitespace) || !repo.contains('/') || repo.contains("..") {
        anyhow::bail!("--repo must be a GitHub owner/name value");
    }
    Ok(repo)
}

fn normalize_nonempty(raw: &str) -> Option<String> {
    let value = raw.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn github_release_asset_url(repo: &str, version: &str, asset_name: &str) -> String {
    if version == "latest" {
        format!("https://github.com/{repo}/releases/latest/download/{asset_name}")
    } else {
        format!("https://github.com/{repo}/releases/download/{version}/{asset_name}")
    }
}

fn release_asset_name(target: &str) -> String {
    format!("rr-{target}")
}

fn asset_name_from_url(url: &str) -> Option<String> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .and_then(normalize_nonempty)
}

fn validate_release_download_urls(
    asset_url: &str,
    checksum_url: Option<&str>,
    has_pinned_sha256: bool,
    allow_insecure_download: bool,
) -> Result<()> {
    if allow_insecure_download {
        return Ok(());
    }

    let checksum_is_trusted = checksum_url
        .map(is_trusted_release_download_url)
        .unwrap_or(false);
    if !is_trusted_release_download_url(asset_url) && !has_pinned_sha256 && !checksum_is_trusted {
        anyhow::bail!(
            "refusing insecure release asset URL {asset_url}; use HTTPS, localhost HTTP, --sha256, or --allow-insecure-download for a trusted private mirror"
        );
    }

    if let Some(checksum_url) = checksum_url
        && !is_trusted_release_download_url(checksum_url)
    {
        anyhow::bail!(
            "refusing insecure checksum URL {checksum_url}; use HTTPS, localhost HTTP, --sha256, or --allow-insecure-download for a trusted private mirror"
        );
    }

    Ok(())
}

fn is_trusted_release_download_url(raw: &str) -> bool {
    let url = raw.trim();
    if url.starts_with("https://") {
        return true;
    }
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let host_port = rest.split('/').next().unwrap_or_default();
    let host_port = host_port.rsplit('@').next().unwrap_or(host_port);
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|addr| addr.is_loopback())
            .unwrap_or(false)
}

fn current_release_target() -> Result<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Ok("aarch64-apple-darwin")
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        Ok("x86_64-apple-darwin")
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Ok("x86_64-unknown-linux-gnu")
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        Ok("aarch64-unknown-linux-gnu")
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64")
    )))]
    {
        anyhow::bail!(
            "unsupported update target: {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }
}

fn download_bytes(url: &str, limit: u64) -> Result<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", concat!("rustory/", env!("CARGO_PKG_VERSION")))
        .call()?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()?;
    Ok(bytes)
}

fn resolve_expected_sha256(request: &UpdateRequest, plan: &UpdatePlan) -> Result<String> {
    if let Some(raw) = request.sha256.as_deref() {
        return normalize_sha256_hex(raw);
    }

    let checksum_url = plan
        .checksum_url
        .as_deref()
        .context("checksum URL unavailable; pass --sha256 explicitly")?;
    let bytes = download_bytes(checksum_url, MAX_CHECKSUM_BYTES)
        .with_context(|| format!("download checksum: {checksum_url}"))?;
    let text = String::from_utf8(bytes).context("checksum response is not utf-8")?;
    parse_sha256_checksum(&text, &plan.asset_name)
}

fn parse_sha256_checksum(text: &str, asset_name: &str) -> Result<String> {
    let mut first_valid = None;
    let mut saw_named_checksum = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        if normalize_sha256_hex(hash).is_err() {
            continue;
        }
        let hash = normalize_sha256_hex(hash)?;
        let names = parts.collect::<Vec<_>>();
        if names.is_empty() {
            first_valid.get_or_insert(hash);
            continue;
        }
        saw_named_checksum = true;
        if names.iter().any(|name| {
            name.trim_start_matches('*')
                .trim_start_matches("./")
                .ends_with(asset_name)
        }) {
            return Ok(hash);
        }
    }

    if saw_named_checksum {
        anyhow::bail!("no SHA-256 checksum found for {asset_name}");
    }
    first_valid.with_context(|| format!("no SHA-256 checksum found for {asset_name}"))
}

fn normalize_sha256_hex(raw: &str) -> Result<String> {
    let value = raw.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("SHA-256 must be exactly 64 hex characters");
    }
    Ok(value)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn verify_sha256(bytes: &[u8], expected: &str) -> Result<()> {
    let expected = normalize_sha256_hex(expected)?;
    let actual = sha256_hex(bytes);
    if actual != expected {
        anyhow::bail!("SHA-256 mismatch: expected {expected}, actual {actual}");
    }
    println!("checksum: ok sha256={actual}");
    Ok(())
}

fn installed_binary_matches(install_path: &Path, bytes: &[u8]) -> Result<bool> {
    match std::fs::read(install_path) {
        Ok(existing) => Ok(existing == bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => {
            Err(err).with_context(|| format!("read installed binary: {}", install_path.display()))
        }
    }
}

fn install_binary(bytes: &[u8], install_path: &Path, requested_version: &str) -> Result<()> {
    let parent = install_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .with_context(|| format!("install path has no parent: {}", install_path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create install dir: {}", parent.display()))?;

    let file_name = install_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("rr");
    let tmp_path = parent.join(format!(".{file_name}.download-{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&tmp_path)
            .with_context(|| format!("create temporary binary: {}", tmp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("write temporary binary: {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temporary binary: {}", tmp_path.display()))?;
    }
    make_executable(&tmp_path)?;
    let result = verify_downloaded_binary(&tmp_path, requested_version).and_then(|()| {
        std::fs::rename(&tmp_path, install_path).with_context(|| {
            format!(
                "replace {} with downloaded binary {}",
                install_path.display(),
                tmp_path.display()
            )
        })
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result.with_context(|| format!("install downloaded binary to {}", install_path.display()))?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DaemonRestartStatus {
    Restarted,
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostUpdateDaemonAction {
    Restart,
    SkipNoRestartDaemon,
}

fn post_update_daemon_action(
    _binary_changed: bool,
    restart_daemon: bool,
) -> PostUpdateDaemonAction {
    if restart_daemon {
        PostUpdateDaemonAction::Restart
    } else {
        PostUpdateDaemonAction::SkipNoRestartDaemon
    }
}

fn handle_post_update_daemon(install_path: &Path, binary_changed: bool, restart_daemon: bool) {
    match post_update_daemon_action(binary_changed, restart_daemon) {
        PostUpdateDaemonAction::Restart => restart_managed_daemon(install_path),
        PostUpdateDaemonAction::SkipNoRestartDaemon => {
            println!("daemon=restart_skipped reason=--no-restart-daemon");
        }
    }
}

fn restart_managed_daemon(install_path: &Path) {
    #[cfg(not(target_os = "linux"))]
    let _ = install_path;

    #[cfg(target_os = "macos")]
    {
        match restart_launchd_daemon() {
            DaemonRestartStatus::Restarted => return,
            DaemonRestartStatus::Failed(error) => {
                println!("warn: daemon restart failed manager=launchd detail={error}");
                return;
            }
            DaemonRestartStatus::Skipped => {}
        }
    }

    #[cfg(target_os = "linux")]
    {
        stop_stale_managed_rr_processes_before_restart(install_path);

        let systemd_bus_unavailable = match restart_systemd_user_daemon() {
            DaemonRestartStatus::Restarted => return,
            DaemonRestartStatus::Failed(error) if systemd_user_bus_unavailable_text(&error) => {
                println!(
                    "daemon=restart_deferred manager=systemd-user reason=user_bus_unavailable"
                );
                true
            }
            DaemonRestartStatus::Failed(error) => {
                println!("warn: daemon restart failed manager=systemd-user detail={error}");
                false
            }
            DaemonRestartStatus::Skipped => false,
        };

        match restart_background_daemon(install_path, systemd_bus_unavailable) {
            DaemonRestartStatus::Restarted => return,
            DaemonRestartStatus::Failed(error) => {
                println!("warn: daemon restart failed manager=background detail={error}");
                return;
            }
            DaemonRestartStatus::Skipped => {}
        }
    }

    println!("daemon=restart_skipped reason=no_managed_daemon_detected");
}

pub fn stop_managed_daemon(install_path: &Path) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    let _ = install_path;

    let mut stopped = false;
    let mut failures: Vec<String> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        match stop_launchd_daemon() {
            DaemonRestartStatus::Restarted => {
                stopped = true;
            }
            DaemonRestartStatus::Failed(error) => {
                println!("warn: daemon stop failed manager=launchd detail={error}");
                failures.push(format!("launchd: {error}"));
            }
            DaemonRestartStatus::Skipped => {}
        }
    }

    #[cfg(target_os = "linux")]
    {
        match stop_systemd_user_daemon() {
            DaemonRestartStatus::Restarted => {
                stopped = true;
            }
            DaemonRestartStatus::Failed(error) if systemd_user_bus_unavailable_text(&error) => {
                println!("daemon=stop_deferred manager=systemd-user reason=user_bus_unavailable");
                failures.push(format!("systemd-user bus unavailable: {error}"));
            }
            DaemonRestartStatus::Failed(error) => {
                println!("warn: daemon stop failed manager=systemd-user detail={error}");
                failures.push(format!("systemd-user: {error}"));
            }
            DaemonRestartStatus::Skipped => {}
        }

        match stop_background_daemon(install_path) {
            DaemonRestartStatus::Restarted => {
                stopped = true;
            }
            DaemonRestartStatus::Failed(error) => {
                println!("warn: daemon stop failed manager=background detail={error}");
                failures.push(format!("background: {error}"));
            }
            DaemonRestartStatus::Skipped => {}
        }
    }

    if !stopped {
        println!("daemon=stop_skipped reason=no_managed_daemon_detected");
    }

    if !failures.is_empty() {
        anyhow::bail!(
            "managed daemon stop incomplete; uninstall aborted before removing files: {}",
            failures.join("; ")
        );
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn stop_background_daemon_for_systemd_transition(install_path: &Path) -> Result<()> {
    match stop_background_daemon(install_path) {
        DaemonRestartStatus::Restarted | DaemonRestartStatus::Skipped => Ok(()),
        DaemonRestartStatus::Failed(error) => anyhow::bail!(
            "failed to stop installer-managed background daemon before systemd-user takeover: {error}"
        ),
    }
}

#[cfg(target_os = "linux")]
fn stop_stale_managed_rr_processes_before_restart(install_path: &Path) {
    match stop_stale_background_rr_processes(install_path) {
        Ok(0) => {}
        Ok(count) => println!("daemon=stale_processes_stopped manager=pre_restart count={count}"),
        Err(err) => {
            println!("warn: daemon stale process cleanup failed manager=pre_restart detail={err}")
        }
    }
}

#[cfg(target_os = "macos")]
fn stop_launchd_daemon() -> DaemonRestartStatus {
    let label = "com.rustory.daemon";
    let uid = unsafe { libc::getuid() };
    let target = format!("gui/{uid}/{label}");
    let plist_path = home_dir()
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist"));
    if !plist_path.exists()
        && !process_status(ProcessCommand::new("launchctl").arg("print").arg(&target))
    {
        return DaemonRestartStatus::Skipped;
    }

    let output = ProcessCommand::new("launchctl")
        .arg("bootout")
        .arg(&target)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            println!("daemon=stopped manager=launchd label={label}");
            DaemonRestartStatus::Restarted
        }
        Ok(output) if launchd_service_missing_output(&output) => {
            println!("daemon=stop_skipped manager=launchd reason=service_not_loaded");
            DaemonRestartStatus::Skipped
        }
        Ok(output) => DaemonRestartStatus::Failed(one_line_output(&output)),
        Err(err) => DaemonRestartStatus::Failed(err.to_string()),
    }
}

#[cfg(target_os = "macos")]
fn launchd_service_missing_output(output: &std::process::Output) -> bool {
    let text = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_ascii_lowercase();
    text.contains("could not find service")
        || text.contains("service is disabled")
        || text.contains("no such process")
        || text.contains("does not exist")
}

#[cfg(target_os = "macos")]
fn restart_launchd_daemon() -> DaemonRestartStatus {
    let label = "com.rustory.daemon";
    let uid = unsafe { libc::getuid() };
    let target = format!("gui/{uid}/{label}");
    let plist_path = home_dir()
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist"));
    if !plist_path.exists()
        && !process_status(ProcessCommand::new("launchctl").arg("print").arg(&target))
    {
        return DaemonRestartStatus::Skipped;
    }

    let output = ProcessCommand::new("launchctl")
        .arg("kickstart")
        .arg("-k")
        .arg(&target)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            println!("daemon=restarted manager=launchd label={label}");
            DaemonRestartStatus::Restarted
        }
        Ok(output) => DaemonRestartStatus::Failed(one_line_output(&output)),
        Err(err) => DaemonRestartStatus::Failed(err.to_string()),
    }
}

#[cfg(target_os = "linux")]
fn stop_systemd_user_daemon() -> DaemonRestartStatus {
    let units = std::iter::once(SYSTEMD_USER_UNIT)
        .chain(LEGACY_SYSTEMD_USER_UNITS.iter().copied())
        .filter(|unit| systemd_user_unit_present(unit))
        .collect::<Vec<_>>();
    if units.is_empty() {
        return DaemonRestartStatus::Skipped;
    }

    for unit in &units {
        if let Err(error) = stop_disable_systemd_user_unit(unit) {
            return DaemonRestartStatus::Failed(error);
        }
        let path = systemd_user_unit_path(unit);
        if path.exists()
            && let Err(error) = std::fs::remove_file(&path)
        {
            return DaemonRestartStatus::Failed(format!(
                "remove systemd unit {}: {error}",
                path.display()
            ));
        }
    }
    if let Err(error) = run_systemctl_user(&["daemon-reload"]) {
        return DaemonRestartStatus::Failed(error);
    }
    println!(
        "daemon=stopped manager=systemd-user units_removed={}",
        units.join(",")
    );
    DaemonRestartStatus::Restarted
}

#[cfg(target_os = "linux")]
fn restart_systemd_user_daemon() -> DaemonRestartStatus {
    let unit_path = systemd_user_unit_path(SYSTEMD_USER_UNIT);
    if !unit_path.exists() {
        return DaemonRestartStatus::Skipped;
    }

    match remove_legacy_systemd_user_units() {
        Ok(0) => {}
        Ok(count) => println!("daemon=legacy_units_removed manager=systemd-user count={count}"),
        Err(error) => return DaemonRestartStatus::Failed(error),
    }

    for args in [
        &["--user", "daemon-reload"][..],
        &["--user", "restart", SYSTEMD_USER_UNIT][..],
    ] {
        if let Err(error) = run_systemctl_user(args) {
            return DaemonRestartStatus::Failed(error);
        }
    }
    println!("daemon=restarted manager=systemd-user unit={SYSTEMD_USER_UNIT}");
    DaemonRestartStatus::Restarted
}

#[cfg(target_os = "linux")]
fn systemd_user_unit_path(unit: &str) -> PathBuf {
    systemd_user_unit_path_for_home(&home_dir(), unit)
}

#[cfg(target_os = "linux")]
fn systemd_user_unit_path_for_home(home: &Path, unit: &str) -> PathBuf {
    home.join(".config/systemd/user").join(unit)
}

#[cfg(target_os = "linux")]
fn systemd_user_unit_present(unit: &str) -> bool {
    systemd_user_unit_path(unit).exists()
        || process_status(ProcessCommand::new("systemctl").args(["--user", "is-active", unit]))
        || process_status(ProcessCommand::new("systemctl").args(["--user", "is-enabled", unit]))
}

#[cfg(target_os = "linux")]
fn run_systemctl_user(args: &[&str]) -> std::result::Result<(), String> {
    match ProcessCommand::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
    {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(one_line_output(&output)),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(target_os = "linux")]
fn stop_disable_systemd_user_unit(unit: &str) -> std::result::Result<(), String> {
    run_systemctl_user(&["stop", unit])?;
    run_systemctl_user(&["disable", unit])
}

#[cfg(target_os = "linux")]
fn remove_legacy_systemd_user_units() -> std::result::Result<usize, String> {
    let mut removed = 0usize;
    for unit in LEGACY_SYSTEMD_USER_UNITS {
        if !systemd_user_unit_present(unit) {
            continue;
        }
        stop_disable_systemd_user_unit(unit)?;
        let path = systemd_user_unit_path(unit);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|error| {
                format!("remove legacy systemd unit {}: {error}", path.display())
            })?;
        }
        removed += 1;
    }
    if removed > 0 {
        run_systemctl_user(&["daemon-reload"])?;
    }
    Ok(removed)
}

#[cfg(target_os = "linux")]
fn stop_background_daemon(install_path: &Path) -> DaemonRestartStatus {
    let state_dir = match rustory_state_dir() {
        Ok(path) => path,
        Err(error) => return DaemonRestartStatus::Failed(error),
    };
    let pid_path = state_dir.join("daemon.pid");
    let mut pid = read_pid_file(&pid_path);
    let mut stopped = false;

    if let Some(candidate) = pid
        && pid_is_running(candidate)
        && !background_pid_matches_install(candidate, install_path)
    {
        println!(
            "warn: daemon pid file is stale; refusing to signal unrelated pid={candidate} path={}",
            pid_path.display()
        );
        let _ = std::fs::remove_file(&pid_path);
        pid = None;
    }

    if let Some(pid) = pid
        && pid_is_running(pid)
    {
        println!("daemon=stopping manager=background pid={pid}");
        if let Err(err) = terminate_background_process(pid, libc::SIGTERM) {
            return DaemonRestartStatus::Failed(format!(
                "terminate background process {pid}: {err}"
            ));
        }
        if !wait_managed_pid_stopped(pid, install_path, Duration::from_secs(5)) {
            let _ = terminate_background_process(pid, libc::SIGKILL);
            if !wait_managed_pid_stopped(pid, install_path, Duration::from_secs(2)) {
                return DaemonRestartStatus::Failed(format!(
                    "pid {pid} did not stop after SIGTERM/SIGKILL"
                ));
            }
        }
        stopped = true;
    }

    match stop_stale_background_rr_processes(install_path) {
        Ok(0) => {}
        Ok(count) => {
            stopped = true;
            println!("daemon=stale_processes_stopped manager=background count={count}");
        }
        Err(err) => {
            return DaemonRestartStatus::Failed(format!("stale process cleanup failed: {err}"));
        }
    }

    let _ = std::fs::remove_file(&pid_path);

    if stopped {
        println!("daemon=stopped manager=background");
        DaemonRestartStatus::Restarted
    } else {
        DaemonRestartStatus::Skipped
    }
}

#[cfg(target_os = "linux")]
fn restart_background_daemon(install_path: &Path, force_start: bool) -> DaemonRestartStatus {
    let state_dir = match rustory_state_dir() {
        Ok(path) => path,
        Err(error) => return DaemonRestartStatus::Failed(error),
    };
    let Some(state_home) = state_dir.parent() else {
        return DaemonRestartStatus::Failed(format!(
            "managed state dir has no parent: {}",
            state_dir.display()
        ));
    };
    let pid_path = state_dir.join("daemon.pid");
    let log_path = state_dir.join("daemon.log");
    let mut pid = read_pid_file(&pid_path);

    if let Some(candidate) = pid
        && pid_is_running(candidate)
        && !background_pid_matches_install(candidate, install_path)
    {
        println!(
            "warn: daemon pid file is stale; refusing to signal unrelated pid={candidate} path={}",
            pid_path.display()
        );
        let _ = std::fs::remove_file(&pid_path);
        pid = None;
    }

    if !force_start && pid.is_none() {
        return DaemonRestartStatus::Skipped;
    }

    if let Some(pid) = pid
        && pid_is_running(pid)
    {
        println!("daemon=stopping manager=background pid={pid}");
        if let Err(err) = terminate_background_process(pid, libc::SIGTERM) {
            return DaemonRestartStatus::Failed(format!(
                "terminate background process {pid}: {err}"
            ));
        }
        if !wait_managed_pid_stopped(pid, install_path, Duration::from_secs(5)) {
            let _ = terminate_background_process(pid, libc::SIGKILL);
            if !wait_managed_pid_stopped(pid, install_path, Duration::from_secs(2)) {
                return DaemonRestartStatus::Failed(format!(
                    "pid {pid} did not stop after SIGTERM/SIGKILL"
                ));
            }
        }
    }

    match stop_stale_background_rr_processes(install_path) {
        Ok(0) => {}
        Ok(count) => println!("daemon=stale_processes_stopped manager=background count={count}"),
        Err(err) => {
            println!("warn: daemon stale process cleanup failed manager=background detail={err}")
        }
    }

    if let Err(err) = std::fs::create_dir_all(&state_dir) {
        return DaemonRestartStatus::Failed(format!(
            "create state dir {}: {err}",
            state_dir.display()
        ));
    }

    let log_file = match OpenOptions::new().create(true).append(true).open(&log_path) {
        Ok(file) => file,
        Err(err) => {
            return DaemonRestartStatus::Failed(format!("open log {}: {err}", log_path.display()));
        }
    };
    let stderr = match log_file.try_clone() {
        Ok(file) => file,
        Err(err) => return DaemonRestartStatus::Failed(format!("clone daemon log: {err}")),
    };

    let mut command = ProcessCommand::new(install_path);
    command
        .arg("daemon")
        .arg("--interval-sec")
        .arg("60")
        .arg("--start-jitter-sec")
        .arg("10")
        .current_dir(home_dir())
        .env("XDG_STATE_HOME", state_home)
        .env("RUSTORY_DAEMON_MANAGER", "background")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr));

    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return DaemonRestartStatus::Failed(format!(
                "spawn {} daemon: {err}",
                install_path.display()
            ));
        }
    };
    let pid = child.id();
    if let Err(err) = std::fs::write(&pid_path, format!("{pid}\n")) {
        return DaemonRestartStatus::Failed(format!("write pid {}: {err}", pid_path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&pid_path, std::fs::Permissions::from_mode(0o600));
    }

    println!(
        "daemon=restarted manager=background pid={pid} log={}",
        log_path.display()
    );
    DaemonRestartStatus::Restarted
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "linux")]
fn rustory_state_dir() -> std::result::Result<PathBuf, String> {
    let state_home = match crate::uninstall::load_managed_state_home() {
        Ok(Some(path)) => path,
        Ok(None) => std::env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".local/state")),
        Err(error) => return Err(format!("load managed state home: {error:#}")),
    };
    if !state_home.is_absolute() {
        return Err(format!(
            "managed XDG_STATE_HOME must be absolute: {}",
            state_home.display()
        ));
    }
    Ok(state_home.join("rustory"))
}

#[cfg(target_os = "linux")]
fn read_pid_file(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0)
}

#[cfg(target_os = "linux")]
fn pid_is_running(pid: u32) -> bool {
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    let err = std::io::Error::last_os_error();
    err.raw_os_error() != Some(libc::ESRCH)
}

#[cfg(target_os = "linux")]
fn terminate_background_process(pid: u32, signal: libc::c_int) -> std::io::Result<()> {
    if pid == 0 {
        return Ok(());
    }

    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    if pgid == pid as libc::pid_t && pgid > 1 {
        let rc = unsafe { libc::kill(-pgid, signal) };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ESRCH) {
            return Err(err);
        }
    }

    let rc = unsafe { libc::kill(pid as libc::pid_t, signal) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(err)
}

#[cfg(target_os = "linux")]
fn stop_stale_background_rr_processes(install_path: &Path) -> std::io::Result<usize> {
    let current_pid = std::process::id();
    let mut child_targets = Vec::new();
    let mut daemon_targets = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if pid == current_pid || !process_is_current_user(pid) {
            continue;
        }
        let Some(cmdline) = read_proc_cmdline(pid) else {
            continue;
        };
        if !process_exe_matches(pid, install_path) || !process_has_background_manager_env(pid) {
            continue;
        }
        match managed_background_cmdline_kind(&cmdline) {
            Some(ManagedBackgroundCmdlineKind::Daemon) => daemon_targets.push(pid),
            Some(ManagedBackgroundCmdlineKind::Child)
                if process_has_managed_daemon_ancestor(pid, install_path) =>
            {
                child_targets.push(pid);
            }
            _ => {}
        }
    }

    child_targets.sort_unstable();
    daemon_targets.sort_unstable();
    let mut targets = child_targets;
    targets.extend(daemon_targets);
    targets.dedup();

    let mut stopped = 0;
    for pid in targets {
        if !managed_process_is_running(pid, install_path) {
            continue;
        }
        terminate_background_process(pid, libc::SIGTERM)?;
        if !wait_managed_process_stopped(pid, install_path, Duration::from_secs(2)) {
            let _ = terminate_background_process(pid, libc::SIGKILL);
            let _ = wait_managed_process_stopped(pid, install_path, Duration::from_secs(1));
        }
        stopped += 1;
    }
    Ok(stopped)
}

#[cfg(target_os = "linux")]
fn process_is_current_user(pid: u32) -> bool {
    let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
        return false;
    };
    let uid = unsafe { libc::getuid() };
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|value| value.parse::<u32>().ok())
        == Some(uid)
}

#[cfg(target_os = "linux")]
fn read_proc_cmdline(pid: u32) -> Option<Vec<String>> {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let parts = bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<Vec<_>>();
    (!parts.is_empty()).then_some(parts)
}

#[cfg(target_os = "linux")]
fn process_exe_matches(pid: u32, install_path: &Path) -> bool {
    let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else {
        return false;
    };
    paths_match_after_deleted_suffix(&exe, install_path)
}

#[cfg(target_os = "linux")]
fn process_has_background_manager_env(pid: u32) -> bool {
    let Ok(environ) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return false;
    };
    environ
        .split(|byte| *byte == 0)
        .any(|entry| entry == b"RUSTORY_DAEMON_MANAGER=background")
}

#[cfg(target_os = "linux")]
fn background_pid_matches_install(pid: u32, install_path: &Path) -> bool {
    if !process_is_current_user(pid) {
        return false;
    }
    let Some(cmdline) = read_proc_cmdline(pid) else {
        return false;
    };
    // The caller obtained this PID from the private installer-owned pid file.
    // Accept that provenance for migration from older updater children that did
    // not yet carry RUSTORY_DAEMON_MANAGER; broad /proc scans require the marker.
    managed_background_cmdline_kind(&cmdline) == Some(ManagedBackgroundCmdlineKind::Daemon)
        && process_exe_matches(pid, install_path)
}

#[cfg(target_os = "linux")]
fn managed_process_matches_install(pid: u32, install_path: &Path) -> bool {
    if !process_is_current_user(pid) {
        return false;
    }
    let Some(cmdline) = read_proc_cmdline(pid) else {
        return false;
    };
    if !process_exe_matches(pid, install_path) || !process_has_background_manager_env(pid) {
        return false;
    }
    match managed_background_cmdline_kind(&cmdline) {
        Some(ManagedBackgroundCmdlineKind::Daemon) => true,
        Some(ManagedBackgroundCmdlineKind::Child) => {
            process_has_managed_daemon_ancestor(pid, install_path)
        }
        None => false,
    }
}

#[cfg(target_os = "linux")]
fn process_has_managed_daemon_ancestor(mut pid: u32, install_path: &Path) -> bool {
    for _ in 0..64 {
        let Some(parent_pid) = read_proc_parent_pid(pid) else {
            return false;
        };
        if parent_pid <= 1 || parent_pid == pid {
            return false;
        }
        let Some(parent_cmdline) = read_proc_cmdline(parent_pid) else {
            return false;
        };
        if managed_background_cmdline_kind(&parent_cmdline)
            == Some(ManagedBackgroundCmdlineKind::Daemon)
            && process_exe_matches(parent_pid, install_path)
            && process_has_background_manager_env(parent_pid)
        {
            return true;
        }
        pid = parent_pid;
    }
    false
}

#[cfg(target_os = "linux")]
fn read_proc_parent_pid(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = stat.rsplit_once(") ")?.1;
    after_comm.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedBackgroundCmdlineKind {
    Daemon,
    Child,
}

#[cfg(any(target_os = "linux", test))]
fn managed_background_cmdline_kind(cmdline: &[String]) -> Option<ManagedBackgroundCmdlineKind> {
    if cmdline.iter().any(|arg| arg == "daemon") {
        return (cmdline.iter().any(|arg| arg == "--interval-sec")
            && cmdline.iter().any(|arg| arg == "--start-jitter-sec"))
        .then_some(ManagedBackgroundCmdlineKind::Daemon);
    }

    if cmdline.iter().any(|arg| arg == "p2p-sync") {
        return (cmdline.iter().any(|arg| arg == "--watch")
            && cmdline_has_default_rustory_db_path(cmdline))
        .then_some(ManagedBackgroundCmdlineKind::Child);
    }
    if cmdline.iter().any(|arg| arg == "p2p-serve") {
        return cmdline_has_default_rustory_db_path(cmdline)
            .then_some(ManagedBackgroundCmdlineKind::Child);
    }
    None
}

#[cfg(test)]
fn is_managed_background_rr_cmdline(cmdline: &[String], has_managed_daemon_ancestor: bool) -> bool {
    match managed_background_cmdline_kind(cmdline) {
        Some(ManagedBackgroundCmdlineKind::Daemon) => true,
        Some(ManagedBackgroundCmdlineKind::Child) => has_managed_daemon_ancestor,
        None => false,
    }
}

#[cfg(any(target_os = "linux", test))]
fn cmdline_has_default_rustory_db_path(cmdline: &[String]) -> bool {
    cmdline
        .windows(2)
        .any(|window| window[0] == "--db-path" && is_default_rustory_db_path(&window[1]))
}

#[cfg(any(target_os = "linux", test))]
fn is_default_rustory_db_path(path: &str) -> bool {
    path == "~/.rustory/history.db"
        || path == "$HOME/.rustory/history.db"
        || path.ends_with("/.rustory/history.db")
}

#[cfg(any(target_os = "linux", test))]
fn paths_match_after_deleted_suffix(left: &Path, right: &Path) -> bool {
    normalize_deleted_exe_path(left) == normalize_deleted_exe_path(right)
}

#[cfg(any(target_os = "linux", test))]
fn normalize_deleted_exe_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    raw.strip_suffix(" (deleted)")
        .unwrap_or(raw.as_ref())
        .to_string()
}

#[cfg(target_os = "linux")]
fn managed_pid_is_running(pid: u32, install_path: &Path) -> bool {
    pid_is_running(pid) && background_pid_matches_install(pid, install_path)
}

#[cfg(target_os = "linux")]
fn managed_process_is_running(pid: u32, install_path: &Path) -> bool {
    pid_is_running(pid) && managed_process_matches_install(pid, install_path)
}

#[cfg(target_os = "linux")]
fn wait_managed_pid_stopped(pid: u32, install_path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if !managed_pid_is_running(pid, install_path) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !managed_pid_is_running(pid, install_path)
}

#[cfg(target_os = "linux")]
fn wait_managed_process_stopped(pid: u32, install_path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if !managed_process_is_running(pid, install_path) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !managed_process_is_running(pid, install_path)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn process_status(command: &mut ProcessCommand) -> bool {
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn one_line_output(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stderr.trim().is_empty() {
        stdout.as_ref()
    } else {
        stderr.as_ref()
    };
    format!(
        "exit={} {}",
        output.status,
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

#[cfg(target_os = "linux")]
fn systemd_user_bus_unavailable_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("failed to connect to bus")
        || text.contains("dbus_session_bus_address")
        || text.contains("xdg_runtime_dir")
        || text.contains("no medium found")
}

fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("chmod 0755: {}", path.display()))?;
    }
    Ok(())
}

fn verify_downloaded_binary(path: &Path, requested_version: &str) -> Result<()> {
    verify_downloaded_binary_with_timeout(path, requested_version, DOWNLOADED_BINARY_CHECK_TIMEOUT)
}

fn verify_downloaded_binary_with_timeout(
    path: &Path,
    requested_version: &str,
    timeout: Duration,
) -> Result<()> {
    let output = run_downloaded_version_with_timeout(path, timeout)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("downloaded binary failed `version`: {stderr}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(expected) = pinned_release_version(requested_version) {
        let actual = parse_rr_version_output(&stdout).with_context(|| {
            format!(
                "downloaded binary did not report a version for pinned release {requested_version}"
            )
        })?;
        if actual != expected {
            anyhow::bail!(
                "downloaded binary version mismatch: requested {expected}, reported {actual}"
            );
        }
    }
    let first_line = stdout
        .lines()
        .next()
        .unwrap_or("version output unavailable");
    println!("downloaded binary check: {first_line}");
    Ok(())
}

fn run_downloaded_version_with_timeout(
    path: &Path,
    timeout: Duration,
) -> Result<std::process::Output> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .context("downloaded binary check timeout is too large")?;
    let nonce = uuid::Uuid::new_v4();
    let output_dir = std::env::temp_dir();
    let stdout_path = output_dir.join(format!("rustory-version-{nonce}.stdout"));
    let stderr_path = output_dir.join(format!("rustory-version-{nonce}.stderr"));
    let stdout_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stdout_path)
        .with_context(|| format!("create version stdout file: {}", stdout_path.display()))?;
    let stderr_file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stderr_path)
    {
        Ok(file) => file,
        Err(err) => {
            let _ = std::fs::remove_file(&stdout_path);
            return Err(err)
                .with_context(|| format!("create version stderr file: {}", stderr_path.display()));
        }
    };

    let mut child = match ProcessCommand::new(path)
        .arg("version")
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            let _ = std::fs::remove_file(&stdout_path);
            let _ = std::fs::remove_file(&stderr_path);
            return Err(err).with_context(|| format!("run downloaded binary: {}", path.display()));
        }
    };

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&stdout_path);
                let _ = std::fs::remove_file(&stderr_path);
                return Err(err)
                    .with_context(|| format!("wait downloaded binary: {}", path.display()));
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&stdout_path);
            let _ = std::fs::remove_file(&stderr_path);
            anyhow::bail!(
                "downloaded binary timed out running `version` after {:.1}s: {}",
                timeout.as_secs_f64(),
                path.display()
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let stdout = std::fs::read(&stdout_path)
        .with_context(|| format!("read version stdout file: {}", stdout_path.display()));
    let stderr = std::fs::read(&stderr_path)
        .with_context(|| format!("read version stderr file: {}", stderr_path.display()));
    let _ = std::fs::remove_file(&stdout_path);
    let _ = std::fs::remove_file(&stderr_path);
    Ok(std::process::Output {
        status,
        stdout: stdout?,
        stderr: stderr?,
    })
}

fn pinned_release_version(requested_version: &str) -> Option<semver::Version> {
    let requested = requested_version.trim();
    if requested == "latest" {
        return None;
    }
    let normalized = requested.strip_prefix('v').unwrap_or(requested);
    semver::Version::parse(normalized).ok()
}

fn parse_rr_version_output(output: &str) -> Option<semver::Version> {
    output.lines().find_map(|line| {
        line.strip_prefix("version:")
            .and_then(|value| semver::Version::parse(value.trim()).ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_plan_defaults_to_github_release_asset() {
        let request = UpdateRequest {
            version: "v1.0.2".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: None,
            asset_url: None,
            checksum_url: None,
            sha256: None,
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
            restart_daemon: true,
            allow_insecure_download: false,
        };

        let plan = build_update_plan(&request).unwrap();
        assert_eq!(plan.version, "v1.0.2");
        assert_eq!(plan.asset_name, release_asset_name(&plan.target));
        assert!(plan.asset_url.contains("/releases/download/v1.0.2/rr-"));
        let expected_checksum_url = format!("{}.sha256", plan.asset_url);
        assert_eq!(
            plan.checksum_url.as_deref(),
            Some(expected_checksum_url.as_str())
        );
        assert_eq!(plan.install_path, PathBuf::from("/tmp/rr"));
    }

    #[test]
    fn update_plan_supports_latest_and_asset_base_url() {
        let request = UpdateRequest {
            version: " latest ".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: Some("https://example.test/releases".to_string()),
            asset_url: None,
            checksum_url: None,
            sha256: Some("0".repeat(64)),
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
            restart_daemon: true,
            allow_insecure_download: false,
        };

        let plan = build_update_plan(&request).unwrap();
        assert_eq!(plan.version, "latest");
        assert!(
            plan.asset_url
                .starts_with("https://example.test/releases/rr-")
        );
        assert!(plan.checksum_url.is_none());
    }

    #[test]
    fn update_plan_rejects_competing_asset_sources() {
        let request = UpdateRequest {
            version: "latest".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: Some("https://example.test/releases".to_string()),
            asset_url: Some("https://example.test/rr".to_string()),
            checksum_url: None,
            sha256: None,
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
            restart_daemon: true,
            allow_insecure_download: false,
        };

        assert!(build_update_plan(&request).is_err());
    }

    #[test]
    fn update_plan_rejects_unpinned_insecure_release_urls() {
        let request = UpdateRequest {
            version: "latest".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: None,
            asset_url: Some("http://example.test/rr".to_string()),
            checksum_url: None,
            sha256: None,
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
            restart_daemon: true,
            allow_insecure_download: false,
        };

        let err = build_update_plan(&request).unwrap_err();
        assert!(format!("{err:#}").contains("refusing insecure release asset URL"));
    }

    #[test]
    fn update_plan_allows_http_loopback_release_urls() {
        let request = UpdateRequest {
            version: "latest".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: Some("http://127.0.0.1:8080/releases".to_string()),
            asset_url: None,
            checksum_url: None,
            sha256: None,
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
            restart_daemon: true,
            allow_insecure_download: false,
        };

        let plan = build_update_plan(&request).unwrap();
        assert!(plan.asset_url.starts_with("http://127.0.0.1:8080/"));
        assert!(
            plan.checksum_url
                .as_deref()
                .unwrap()
                .starts_with("http://127.0.0.1:8080/")
        );
    }

    #[test]
    fn update_plan_allows_pinned_insecure_asset_without_checksum_url() {
        let request = UpdateRequest {
            version: "latest".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: None,
            asset_url: Some("http://example.test/rr".to_string()),
            checksum_url: None,
            sha256: Some("0".repeat(64)),
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
            restart_daemon: true,
            allow_insecure_download: false,
        };

        let plan = build_update_plan(&request).unwrap();
        assert_eq!(plan.asset_url, "http://example.test/rr");
        assert!(plan.checksum_url.is_none());
    }

    #[test]
    fn update_plan_allows_explicit_insecure_download_override() {
        let request = UpdateRequest {
            version: "latest".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: Some("http://example.test/releases".to_string()),
            asset_url: None,
            checksum_url: None,
            sha256: None,
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
            restart_daemon: true,
            allow_insecure_download: true,
        };

        let plan = build_update_plan(&request).unwrap();
        assert!(
            plan.asset_url
                .starts_with("http://example.test/releases/rr-")
        );
        assert!(
            plan.checksum_url
                .as_deref()
                .unwrap()
                .starts_with("http://example.test/releases/rr-")
        );
    }

    #[test]
    fn checksum_parser_accepts_raw_and_file_format() {
        let raw = "a".repeat(64);
        assert_eq!(parse_sha256_checksum(&raw, "rr-test").unwrap(), raw);

        let text = format!("{}  rr-test\n{}  other\n", "b".repeat(64), "c".repeat(64));
        assert_eq!(
            parse_sha256_checksum(&text, "rr-test").unwrap(),
            "b".repeat(64)
        );
    }

    #[test]
    fn checksum_parser_rejects_missing_named_asset() {
        let text = format!("{}  other\n", "b".repeat(64));
        assert!(parse_sha256_checksum(&text, "rr-test").is_err());
    }

    #[test]
    fn verify_sha256_detects_mismatch() {
        let expected = sha256_hex(b"hello");
        assert!(verify_sha256(b"hello", &expected).is_ok());
        assert!(verify_sha256(b"hello", &"0".repeat(64)).is_err());
    }

    #[test]
    fn pinned_release_version_accepts_semver_and_preserves_custom_tags() {
        assert_eq!(
            pinned_release_version("v1.2.3"),
            Some(semver::Version::new(1, 2, 3))
        );
        assert_eq!(
            pinned_release_version("1.2.3-beta.1"),
            Some(semver::Version::parse("1.2.3-beta.1").unwrap())
        );
        assert_eq!(pinned_release_version("latest"), None);
        assert_eq!(pinned_release_version("nightly-main"), None);
    }

    #[cfg(unix)]
    #[test]
    fn downloaded_binary_must_match_pinned_version_but_not_custom_tag() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("rr");
        std::fs::write(
            &binary,
            "#!/bin/sh\nprintf 'version: 9.9.9\\nrevision: test\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(verify_downloaded_binary(&binary, "v9.9.9").is_ok());
        let err = verify_downloaded_binary(&binary, "v1.2.3").unwrap_err();
        assert!(format!("{err:#}").contains("version mismatch"));
        assert!(verify_downloaded_binary(&binary, "latest").is_ok());
        assert!(verify_downloaded_binary(&binary, "nightly-main").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn downloaded_binary_version_check_times_out() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("rr");
        std::fs::write(&binary, "#!/bin/sh\nwhile :; do :; done\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

        let err =
            verify_downloaded_binary_with_timeout(&binary, "latest", Duration::from_millis(50))
                .unwrap_err();
        assert!(format!("{err:#}").contains("timed out running `version`"));
    }

    #[test]
    fn installed_binary_match_detects_identical_bytes() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), b"rr-binary").unwrap();

        assert!(installed_binary_matches(temp.path(), b"rr-binary").unwrap());
        assert!(!installed_binary_matches(temp.path(), b"other").unwrap());
    }

    #[test]
    fn installed_binary_match_treats_missing_as_changed() {
        let path = std::env::temp_dir().join(format!(
            "rustory-missing-update-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        assert!(!installed_binary_matches(&path, b"rr-binary").unwrap());
    }

    #[test]
    fn deleted_exe_path_normalization_matches_current_install_path() {
        assert!(paths_match_after_deleted_suffix(
            Path::new("/home/user/.local/bin/rr (deleted)"),
            Path::new("/home/user/.local/bin/rr")
        ));
        assert!(!paths_match_after_deleted_suffix(
            Path::new("/home/user/.local/bin/other"),
            Path::new("/home/user/.local/bin/rr")
        ));
    }

    #[test]
    fn managed_background_cmdline_matches_only_daemon_children() {
        assert!(is_managed_background_rr_cmdline(
            &[
                "/home/user/.local/bin/rr".to_string(),
                "daemon".to_string(),
                "--interval-sec".to_string(),
                "60".to_string(),
                "--start-jitter-sec".to_string(),
                "10".to_string(),
            ],
            false
        ));
        assert!(is_managed_background_rr_cmdline(
            &[
                "/home/user/.local/bin/rr".to_string(),
                "--db-path".to_string(),
                "/home/user/.rustory/history.db".to_string(),
                "p2p-sync".to_string(),
                "--watch".to_string(),
            ],
            true
        ));
        assert!(is_managed_background_rr_cmdline(
            &[
                "/home/user/.local/bin/rr".to_string(),
                "--db-path".to_string(),
                "/home/user/.rustory/history.db".to_string(),
                "p2p-serve".to_string(),
            ],
            true
        ));
        assert!(!is_managed_background_rr_cmdline(
            &[
                "/home/user/.local/bin/rr".to_string(),
                "--db-path".to_string(),
                "/home/user/.rustory/history.db".to_string(),
                "p2p-serve".to_string(),
            ],
            false
        ));
        assert!(!is_managed_background_rr_cmdline(
            &[
                "/home/user/.local/bin/rr".to_string(),
                "p2p-serve".to_string(),
            ],
            false
        ));
        assert!(!is_managed_background_rr_cmdline(
            &[
                "/home/user/.local/bin/rr".to_string(),
                "p2p-sync".to_string(),
            ],
            false
        ));
        assert!(!is_managed_background_rr_cmdline(
            &[
                "/home/user/.local/bin/rr".to_string(),
                "sync-status".to_string(),
                "--with-tracker".to_string(),
            ],
            false
        ));
    }

    #[test]
    fn managed_background_cmdline_requires_installer_daemon_signature() {
        assert!(is_managed_background_rr_cmdline(
            &[
                "rr".to_string(),
                "--db-path".to_string(),
                "~/.rustory/history.db".to_string(),
                "p2p-sync".to_string(),
                "--watch".to_string(),
                "--max-peers-per-tick".to_string(),
                "1".to_string(),
            ],
            true
        ));
        assert!(!is_managed_background_rr_cmdline(
            &[
                "/home/user/.local/bin/rr".to_string(),
                "daemon".to_string(),
                "--interval-sec".to_string(),
                "60".to_string(),
            ],
            false
        ));
        assert!(is_managed_background_rr_cmdline(
            &[
                "rr".to_string(),
                "daemon".to_string(),
                "--preflight".to_string(),
                "--interval-sec".to_string(),
                "60".to_string(),
                "--start-jitter-sec".to_string(),
                "10".to_string(),
            ],
            false
        ));
        assert!(!is_managed_background_rr_cmdline(
            &[
                "rr".to_string(),
                "sync-status".to_string(),
                "--watch".to_string(),
            ],
            false
        ));
        assert!(!is_managed_background_rr_cmdline(
            &[
                "rr".to_string(),
                "--db-path".to_string(),
                "/tmp/other.db".to_string(),
                "p2p-sync".to_string(),
                "--watch".to_string(),
            ],
            true
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unrelated_running_pid_never_matches_managed_background_daemon() {
        let mut child = ProcessCommand::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let install_path = std::env::current_exe().unwrap();

        assert!(pid_is_running(pid));
        assert!(!background_pid_matches_install(pid, &install_path));
        assert!(pid_is_running(pid));

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn post_update_restart_runs_even_when_binary_is_unchanged() {
        assert_eq!(
            post_update_daemon_action(false, true),
            PostUpdateDaemonAction::Restart
        );
        assert_eq!(
            post_update_daemon_action(true, true),
            PostUpdateDaemonAction::Restart
        );
        assert_eq!(
            post_update_daemon_action(false, false),
            PostUpdateDaemonAction::SkipNoRestartDaemon
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_user_bus_detection_accepts_container_messages() {
        assert!(systemd_user_bus_unavailable_text(
            "Failed to connect to bus: $DBUS_SESSION_BUS_ADDRESS and $XDG_RUNTIME_DIR not defined"
        ));
        assert!(!systemd_user_bus_unavailable_text(
            "Unit rustory.service not found"
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_user_unit_paths_include_legacy_daemon_unit() {
        let home = Path::new("/home/user");

        assert_eq!(
            systemd_user_unit_path_for_home(home, SYSTEMD_USER_UNIT),
            home.join(".config/systemd/user/rustory.service")
        );
        assert_eq!(LEGACY_SYSTEMD_USER_UNITS, &["rustory-daemon.service"]);
        assert_eq!(
            systemd_user_unit_path_for_home(home, LEGACY_SYSTEMD_USER_UNITS[0]),
            home.join(".config/systemd/user/rustory-daemon.service")
        );
    }
}
