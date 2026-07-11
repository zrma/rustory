use anyhow::{Context, Result};
use rand::TryRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const RETIREMENT_PROTOCOL_VERSION: u32 = 1;
pub const DEVICE_MEMBERSHIP_PROTOCOL_VERSION: u32 = 1;
pub const DEVICE_PROOF_MAX_SKEW_SEC: i64 = 5 * 60;
const DEVICE_PROOF_MAX_PUBLIC_KEY_BYTES: usize = 1024;
const DEVICE_PROOF_MAX_SIGNATURE_BYTES: usize = 1024;
const DEVICE_PROOF_MAX_NONCE_BYTES: usize = 64;
const RETIREMENT_JOB_VERSION: u32 = 3;
const RETIREMENT_POLL_INTERVAL: Duration = Duration::from_secs(30);
const RETIREMENT_HELPER_ENV: &str = "RUSTORY_RETIREMENT_HELPER";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceProof {
    pub peer_id: String,
    pub issued_at_unix: i64,
    pub nonce: String,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetirementCleanup {
    RevokeOnly,
    FullUninstall,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetirementStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Refused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetirementTicket {
    pub ticket_id: String,
    pub peer_id: String,
    pub device_id: Option<String>,
    pub user_id: Option<String>,
    pub cleanup: RetirementCleanup,
    pub issued_at_unix: i64,
    pub status: RetirementStatus,
    pub status_updated_at_unix: i64,
    pub status_detail: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetirementJob {
    version: u32,
    pub tracker_url: String,
    install_path: PathBuf,
    identity_key_path: PathBuf,
    pub ticket: RetirementTicket,
    cleanup_plan: Option<RetirementCleanupPlan>,
    completion_capability: Option<String>,
    running_accepted: bool,
    cleanup_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetirementCleanupPlan {
    pub install_path: PathBuf,
    pub config_path: PathBuf,
    pub db_path: PathBuf,
    pub config_key_paths: Vec<PathBuf>,
    pub state_marker_paths: Vec<PathBuf>,
    pub extra_rc_files: Vec<PathBuf>,
    pub local_peer_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetirementSchedule {
    Scheduled,
    AlreadyScheduled,
}

pub fn validate_retirement_tracker_url(base_url: &str) -> Result<()> {
    let base_url = base_url.trim();
    if base_url.starts_with("https://") {
        return Ok(());
    }
    #[cfg(test)]
    if is_exact_loopback_http_url(base_url) {
        return Ok(());
    }
    anyhow::bail!("remote retirement requires an HTTPS tracker (loopback HTTP is test-only)")
}

pub fn validate_admin_tracker_url(base_url: &str) -> Result<()> {
    let base_url = base_url.trim();
    if base_url.starts_with("https://") {
        return Ok(());
    }
    #[cfg(test)]
    if is_exact_loopback_http_url(base_url) {
        return Ok(());
    }
    anyhow::bail!("device administration requires an HTTPS tracker (loopback HTTP is test-only)")
}

#[cfg(test)]
fn is_exact_loopback_http_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    let (host, port) = if let Some(ipv6) = authority.strip_prefix('[') {
        let Some((host, suffix)) = ipv6.split_once(']') else {
            return false;
        };
        let port = if suffix.is_empty() {
            None
        } else if let Some(port) = suffix.strip_prefix(':') {
            Some(port)
        } else {
            return false;
        };
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host, Some(port))
    } else {
        (authority, None)
    };
    if port.is_some_and(|port| port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit())) {
        return false;
    }
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

pub fn spawn_retirement_monitor(
    stop: Arc<AtomicBool>,
    tracker_url: String,
    identity: crate::libp2p::identity::Keypair,
    identity_key_path: PathBuf,
    executable: PathBuf,
    cleanup_plan: RetirementCleanupPlan,
) -> Result<()> {
    validate_retirement_tracker_url(&tracker_url)?;
    anyhow::ensure!(
        executable.is_absolute(),
        "retirement helper executable must be absolute"
    );
    anyhow::ensure!(
        identity_key_path.is_absolute(),
        "retirement identity key path must be absolute"
    );
    std::thread::Builder::new()
        .name("rustory-retirement-monitor".to_string())
        .spawn(move || {
            // poll/ack are authenticated by the enrolled device proof. Keeping the shared
            // fleet bearer token out of the recovery path also makes token rotation safe.
            let client = crate::tracker::TrackerClient::new(tracker_url.clone(), None);
            while !stop.load(Ordering::SeqCst) {
                match client.poll_retirement(&identity) {
                    Ok(response) => {
                        let Some(ticket) = response.ticket else {
                            if sleep_until_stopped(RETIREMENT_POLL_INTERVAL, stop.as_ref()) {
                                return;
                            }
                            continue;
                        };
                        if ticket.cleanup != RetirementCleanup::FullUninstall {
                            // Revoke-only can later be upgraded to a new full-uninstall ticket.
                            // Keep the monitor alive so an online target observes that upgrade
                            // without requiring a daemon restart.
                        } else if let Err(error) = validate_retirement_ticket(&ticket, &identity) {
                            eprintln!("warn: ignoring invalid retirement ticket: {error:#}");
                        } else if matches!(
                            ticket.status,
                            RetirementStatus::Pending | RetirementStatus::Running
                        ) {
                            let mut receipt_ticket = ticket.clone();
                            receipt_ticket.status = RetirementStatus::Pending;
                            receipt_ticket.status_detail = None;
                            let job = RetirementJob {
                                version: RETIREMENT_JOB_VERSION,
                                tracker_url: tracker_url.clone(),
                                install_path: executable.clone(),
                                identity_key_path: identity_key_path.clone(),
                                ticket: receipt_ticket,
                                cleanup_plan: Some(cleanup_plan.clone()),
                                completion_capability: None,
                                running_accepted: false,
                                cleanup_completed: false,
                            };
                            match schedule_retirement_job(&job, &executable, std::process::id()) {
                                Ok(RetirementSchedule::Scheduled) => {
                                    eprintln!(
                                        "device retirement: helper scheduled ticket_id={}",
                                        ticket.ticket_id
                                    );
                                }
                                Ok(RetirementSchedule::AlreadyScheduled) => {
                                    eprintln!(
                                        "device retirement: helper already scheduled ticket_id={}",
                                        ticket.ticket_id
                                    );
                                }
                                Err(error) => {
                                    let detail = bounded_status_detail(&format!(
                                        "helper scheduling failed: {error:#}"
                                    ));
                                    match client.acknowledge_retirement(
                                        &identity,
                                        ticket.ticket_id.clone(),
                                        RetirementStatus::Failed,
                                        Some(detail),
                                        None,
                                    ) {
                                        Ok(response) if response.ok => {}
                                        Ok(_) => eprintln!(
                                            "warn: retirement helper scheduling failed and failure ACK returned ok=false: schedule={error:#}"
                                        ),
                                        Err(ack_error) => eprintln!(
                                            "warn: retirement helper scheduling and failure ACK both failed: schedule={error:#} ack={ack_error:#}"
                                        ),
                                    }
                                    if sleep_until_stopped(
                                        RETIREMENT_POLL_INTERVAL,
                                        stop.as_ref(),
                                    ) {
                                        return;
                                    }
                                    continue;
                                }
                            }
                        } else if retirement_ticket_is_terminal_for_monitor(&ticket) {
                            return;
                        }
                    }
                    Err(error) => {
                        eprintln!("warn: retirement ticket poll failed: {error:#}");
                    }
                }

                if sleep_until_stopped(RETIREMENT_POLL_INTERVAL, stop.as_ref()) {
                    return;
                }
            }
        })
        .context("spawn retirement monitor")?;
    Ok(())
}

fn retirement_ticket_is_terminal_for_monitor(ticket: &RetirementTicket) -> bool {
    ticket.cleanup == RetirementCleanup::FullUninstall
        && ticket.status == RetirementStatus::Completed
}

pub fn retirement_helper_invocation_is_authorized() -> bool {
    std::env::var_os(RETIREMENT_HELPER_ENV).is_some_and(|value| value == "1")
}

pub fn managed_retirement_executor_matches_process(process_id: u32) -> Result<bool> {
    #[cfg(target_os = "macos")]
    return Ok(launchd_daemon_pid()?.is_some_and(|pid| pid == process_id));
    #[cfg(target_os = "linux")]
    return Ok(active_systemd_daemon_unit(process_id)?.is_some());
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = process_id;
        Ok(false)
    }
}

pub fn load_retirement_job(job_id: &str) -> Result<RetirementJob> {
    let job_id = normalize_ticket_id(job_id)?;
    let path = retirement_job_path(&job_id)?;
    let metadata = std::fs::symlink_metadata(&path)
        .with_context(|| format!("inspect retirement job: {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "retirement job must be a regular non-symlink file: {}",
        path.display()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        anyhow::ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "retirement job permissions are too broad: {}",
            path.display()
        );
    }
    let bytes =
        std::fs::read(&path).with_context(|| format!("read retirement job: {}", path.display()))?;
    let job: RetirementJob = serde_json::from_slice(&bytes).context("parse retirement job json")?;
    anyhow::ensure!(
        job.version == RETIREMENT_JOB_VERSION,
        "unsupported retirement job version"
    );
    anyhow::ensure!(job.ticket.ticket_id == job_id, "retirement job id mismatch");
    validate_retirement_tracker_url(&job.tracker_url)?;
    validate_retirement_ticket_shape(&job.ticket)?;
    anyhow::ensure!(
        job.install_path.is_absolute(),
        "retirement install path must be absolute"
    );
    if let Some(plan) = job.cleanup_plan.as_ref() {
        validate_cleanup_plan(plan, &job)?;
    }
    validate_retirement_ticket_for_storage(&job)?;
    Ok(job)
}

impl RetirementJob {
    pub fn install_path(&self) -> &Path {
        &self.install_path
    }

    pub fn identity_key_path(&self) -> &Path {
        &self.identity_key_path
    }

    pub fn cleanup_plan(&self) -> Option<&RetirementCleanupPlan> {
        self.cleanup_plan.as_ref()
    }

    pub fn cleanup_completed(&self) -> bool {
        self.cleanup_completed
    }

    pub fn running_accepted(&self) -> bool {
        self.running_accepted
    }

    pub fn completion_capability(&self) -> Option<&str> {
        self.completion_capability.as_deref()
    }
}

pub fn store_retirement_acceptance(
    job_id: &str,
    plan: RetirementCleanupPlan,
    completion_capability: String,
) -> Result<RetirementJob> {
    let mut job = load_retirement_job(job_id)?;
    validate_cleanup_plan(&plan, &job)?;
    validate_completion_capability(&completion_capability)?;
    match (
        job.cleanup_plan.as_ref(),
        job.completion_capability.as_deref(),
    ) {
        (Some(existing_plan), Some(existing_capability)) => {
            anyhow::ensure!(
                existing_plan == &plan && existing_capability == completion_capability,
                "retirement acceptance changed after it was persisted"
            );
            return Ok(job);
        }
        (Some(existing_plan), None) => {
            anyhow::ensure!(
                existing_plan == &plan,
                "retirement cleanup plan changed after ticket receipt"
            );
        }
        (None, None) => {
            job.cleanup_plan = Some(plan);
        }
        (None, Some(_)) => {
            anyhow::bail!("retirement receipt contains a capability without a cleanup plan")
        }
    }
    job.completion_capability = Some(completion_capability);
    write_retirement_job(&job)?;
    Ok(job)
}

pub fn mark_retirement_cleanup_completed(job_id: &str) -> Result<RetirementJob> {
    let mut job = load_retirement_job(job_id)?;
    anyhow::ensure!(
        job.cleanup_plan.is_some(),
        "retirement cleanup plan is missing"
    );
    if !job.cleanup_completed {
        job.cleanup_completed = true;
        write_retirement_job(&job)?;
    }
    Ok(job)
}

pub fn mark_retirement_running_accepted(job_id: &str) -> Result<RetirementJob> {
    let mut job = load_retirement_job(job_id)?;
    anyhow::ensure!(
        job.cleanup_plan.is_some() && job.completion_capability.is_some(),
        "retirement acceptance record is missing"
    );
    if !job.running_accepted {
        job.running_accepted = true;
        write_retirement_job(&job)?;
    }
    Ok(job)
}

fn write_retirement_job(job: &RetirementJob) -> Result<()> {
    validate_retirement_ticket_for_storage(job)?;
    let path = retirement_job_path(&job.ticket.ticket_id)?;
    let bytes = serde_json::to_vec_pretty(job).context("serialize retirement job")?;
    crate::config::write_private_file(&path, &bytes, true)
        .with_context(|| format!("persist retirement job: {}", path.display()))
}

fn validate_cleanup_plan(plan: &RetirementCleanupPlan, job: &RetirementJob) -> Result<()> {
    for (label, path) in [
        ("install", &plan.install_path),
        ("config", &plan.config_path),
        ("database", &plan.db_path),
    ] {
        anyhow::ensure!(
            path.is_absolute(),
            "retirement {label} path must be absolute"
        );
    }
    for path in plan
        .config_key_paths
        .iter()
        .chain(plan.state_marker_paths.iter())
        .chain(plan.extra_rc_files.iter())
    {
        anyhow::ensure!(
            path.is_absolute(),
            "retirement cleanup paths must be absolute"
        );
    }
    let mut cleanup_paths = vec![
        plan.install_path.clone(),
        plan.config_path.clone(),
        plan.db_path.clone(),
    ];
    let mut db_wal = plan.db_path.as_os_str().to_os_string();
    db_wal.push("-wal");
    cleanup_paths.push(PathBuf::from(db_wal));
    let mut db_shm = plan.db_path.as_os_str().to_os_string();
    db_shm.push("-shm");
    cleanup_paths.push(PathBuf::from(db_shm));
    cleanup_paths.extend(plan.config_key_paths.iter().cloned());
    cleanup_paths.extend(plan.state_marker_paths.iter().cloned());
    cleanup_paths.extend(plan.extra_rc_files.iter().cloned());
    let protected_paths = [
        retirement_job_path(&job.ticket.ticket_id)?,
        retirement_helper_path(&job.ticket.ticket_id)?,
    ];
    for path in &cleanup_paths {
        anyhow::ensure!(
            !path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            }),
            "retirement cleanup path must not contain . or .. components: {}",
            path.display()
        );
        anyhow::ensure!(
            !protected_paths.iter().any(|protected| protected == path),
            "retirement cleanup path overlaps its recovery receipt: {}",
            path.display()
        );
    }
    anyhow::ensure!(
        plan.install_path == job.install_path,
        "retirement cleanup install path does not match local receipt"
    );
    anyhow::ensure!(
        plan.local_peer_id == job.ticket.peer_id,
        "retirement cleanup identity does not match ticket"
    );
    Ok(())
}

pub fn generate_completion_capability() -> Result<(String, String)> {
    let mut bytes = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .context("generate retirement completion capability")?;
    let capability = hex_lower(&bytes);
    let hash = completion_capability_hash(&capability)?;
    Ok((capability, hash))
}

pub fn completion_capability_hash(capability: &str) -> Result<String> {
    validate_completion_capability(capability)?;
    Ok(hex_lower(&Sha256::digest(capability.as_bytes())))
}

fn validate_completion_capability(capability: &str) -> Result<()> {
    anyhow::ensure!(
        capability.len() == 64
            && capability
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "retirement completion capability must be 32-byte lowercase hex"
    );
    Ok(())
}

pub fn validate_retirement_ticket(
    ticket: &RetirementTicket,
    identity: &crate::libp2p::identity::Keypair,
) -> Result<()> {
    validate_retirement_ticket_shape(ticket)?;
    let peer_id: crate::libp2p::PeerId = ticket
        .peer_id
        .parse()
        .context("parse retirement ticket peer_id")?;
    anyhow::ensure!(
        peer_id == identity.public().to_peer_id(),
        "retirement ticket targets a different identity"
    );
    Ok(())
}

fn validate_retirement_ticket_shape(ticket: &RetirementTicket) -> Result<()> {
    let ticket_id = normalize_ticket_id(&ticket.ticket_id)?;
    anyhow::ensure!(
        ticket_id == ticket.ticket_id,
        "retirement ticket id is not canonical"
    );
    ticket
        .peer_id
        .parse::<crate::libp2p::PeerId>()
        .context("parse retirement ticket peer_id")?;
    anyhow::ensure!(
        ticket.cleanup == RetirementCleanup::FullUninstall,
        "retirement helper only accepts full_uninstall tickets"
    );
    anyhow::ensure!(
        ticket
            .device_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            && ticket
                .user_id
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()),
        "retirement ticket requires device_id and user_id bindings"
    );
    Ok(())
}

pub fn remove_retirement_job(job_id: &str) -> Result<()> {
    let job_id = normalize_ticket_id(job_id)?;
    remove_retirement_helper_artifact(&job_id)?;
    remove_regular_file_if_present(&retirement_helper_path(&job_id)?)?;
    let path = retirement_job_path(&job_id)?;
    remove_regular_file_if_present(&path)?;
    if let Some(parent) = path.parent() {
        match std::fs::remove_dir(parent) {
            Ok(()) => {
                if let Some(grandparent) = parent.parent() {
                    sync_directory(grandparent)?;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("remove retirement dir: {}", parent.display()));
            }
        }
    }
    remove_empty_retirement_state_dir(&rustory_state_dir()?)?;
    Ok(())
}

fn remove_empty_retirement_state_dir(path: &Path) -> Result<()> {
    match std::fs::remove_dir(path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                sync_directory(parent)?;
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("remove empty retirement state dir: {}", path.display()));
        }
    }
    Ok(())
}

pub fn remove_retirement_helper_artifact(job_id: &str) -> Result<()> {
    let job_id = normalize_ticket_id(job_id)?;
    cleanup_retirement_helper_artifact(&job_id)
}

pub fn bounded_status_detail(detail: &str) -> String {
    let mut output = String::new();
    for ch in detail.chars() {
        let ch = if ch.is_control() { ' ' } else { ch };
        if output.len().saturating_add(ch.len_utf8()) > 240 {
            break;
        }
        output.push(ch);
    }
    output
}

fn schedule_retirement_job(
    job: &RetirementJob,
    executable: &Path,
    daemon_pid: u32,
) -> Result<RetirementSchedule> {
    validate_retirement_ticket_for_storage(job)?;
    let job_path = retirement_job_path(&job.ticket.ticket_id)?;
    let mut created = false;
    let existing = match std::fs::symlink_metadata(&job_path) {
        Ok(_) => Some(load_retirement_job(&job.ticket.ticket_id)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let bytes = serde_json::to_vec_pretty(job).context("serialize retirement job")?;
            match crate::config::write_private_file(&job_path, &bytes, false) {
                Ok(()) => {
                    created = true;
                    None
                }
                Err(error) if job_path.exists() => {
                    Some(load_retirement_job(&job.ticket.ticket_id).with_context(|| {
                        format!("load concurrently-created retirement job: {error:#}")
                    })?)
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect retirement job: {}", job_path.display()));
        }
    };
    if let Some(existing) = existing.as_ref() {
        anyhow::ensure!(
            same_retirement_request(existing, job),
            "existing retirement job does not match immutable ticket fields"
        );
    }

    let helper = prepare_retirement_helper_copy(executable, &job.ticket.ticket_id)?;
    schedule_platform_helper(&helper, &job.ticket.ticket_id, daemon_pid)?;
    Ok(if created {
        RetirementSchedule::Scheduled
    } else {
        RetirementSchedule::AlreadyScheduled
    })
}

fn same_retirement_request(existing: &RetirementJob, requested: &RetirementJob) -> bool {
    existing.version == requested.version
        && existing.tracker_url.trim_end_matches('/') == requested.tracker_url.trim_end_matches('/')
        && existing.install_path == requested.install_path
        && existing.identity_key_path == requested.identity_key_path
        && existing.ticket.ticket_id == requested.ticket.ticket_id
        && existing.ticket.peer_id == requested.ticket.peer_id
        && existing.ticket.device_id == requested.ticket.device_id
        && existing.ticket.user_id == requested.ticket.user_id
        && existing.ticket.cleanup == requested.ticket.cleanup
        && existing.ticket.issued_at_unix == requested.ticket.issued_at_unix
}

fn validate_retirement_ticket_for_storage(job: &RetirementJob) -> Result<()> {
    anyhow::ensure!(
        job.version == RETIREMENT_JOB_VERSION,
        "invalid retirement job version"
    );
    normalize_ticket_id(&job.ticket.ticket_id)?;
    validate_retirement_tracker_url(&job.tracker_url)?;
    anyhow::ensure!(
        job.ticket.cleanup == RetirementCleanup::FullUninstall,
        "only full_uninstall tickets may schedule a helper"
    );
    anyhow::ensure!(
        job.identity_key_path.is_absolute()
            && !job.identity_key_path.components().any(|component| matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )),
        "retirement identity key path must be absolute and normalized"
    );
    anyhow::ensure!(
        job.ticket.status == RetirementStatus::Pending,
        "only pending retirement tickets may schedule a helper"
    );
    match (
        job.cleanup_plan.as_ref(),
        job.completion_capability.as_deref(),
    ) {
        (Some(plan), Some(capability)) => {
            validate_cleanup_plan(plan, job)?;
            validate_completion_capability(capability)?;
        }
        (Some(plan), None) => validate_cleanup_plan(plan, job)?,
        (None, None) => {}
        (None, Some(_)) => {
            anyhow::bail!("retirement receipt contains a capability without a cleanup plan")
        }
    }
    anyhow::ensure!(
        !job.cleanup_completed || job.cleanup_plan.is_some(),
        "completed cleanup receipt is missing its cleanup plan"
    );
    anyhow::ensure!(
        !job.running_accepted
            || (job.cleanup_plan.is_some() && job.completion_capability.is_some()),
        "accepted retirement receipt is missing its cleanup plan or completion capability"
    );
    anyhow::ensure!(
        !job.cleanup_completed || job.running_accepted,
        "completed cleanup receipt was never accepted by the tracker"
    );
    Ok(())
}

fn normalize_ticket_id(ticket_id: &str) -> Result<String> {
    let parsed = uuid::Uuid::parse_str(ticket_id.trim()).context("invalid retirement ticket id")?;
    let canonical = parsed.to_string();
    anyhow::ensure!(
        ticket_id.trim() == canonical,
        "retirement ticket id must be canonical UUID"
    );
    Ok(canonical)
}

fn retirement_job_path(job_id: &str) -> Result<PathBuf> {
    Ok(rustory_state_dir()?
        .join("retirement")
        .join(format!("{job_id}.json")))
}

fn retirement_helper_path(job_id: &str) -> Result<PathBuf> {
    let job_id = normalize_ticket_id(job_id)?;
    Ok(rustory_state_dir()?
        .join("retirement")
        .join(format!("{job_id}.rr")))
}

fn rustory_state_dir() -> Result<PathBuf> {
    Ok(resolved_state_home()?.join("rustory"))
}

fn resolved_state_home() -> Result<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => {
            let home = PathBuf::from(std::env::var_os("HOME").context("HOME env var not set")?);
            anyhow::ensure!(
                home.is_absolute(),
                "HOME must be absolute for retirement jobs"
            );
            home.join(".local/state")
        }
    };
    anyhow::ensure!(
        base.is_absolute(),
        "XDG_STATE_HOME must be absolute for retirement jobs"
    );
    Ok(base)
}

fn prepare_retirement_helper_copy(executable: &Path, job_id: &str) -> Result<PathBuf> {
    anyhow::ensure!(
        executable.is_absolute(),
        "retirement executable must be absolute"
    );
    require_regular_file(executable, "retirement source executable")?;
    let helper = retirement_helper_path(job_id)?;
    match std::fs::symlink_metadata(&helper) {
        Ok(_) => repair_or_validate_private_helper_executable(&helper, executable)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let bytes = std::fs::read(executable).with_context(|| {
                format!(
                    "read retirement source executable: {}",
                    executable.display()
                )
            })?;
            anyhow::ensure!(!bytes.is_empty(), "retirement source executable is empty");
            if let Err(error) = crate::config::write_private_file(&helper, &bytes, false) {
                if helper.exists() {
                    repair_or_validate_private_helper_executable(&helper, executable)
                        .with_context(|| {
                            format!("validate concurrently-created helper: {error:#}")
                        })?;
                } else {
                    return Err(error);
                }
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))
                    .with_context(|| format!("chmod retirement helper: {}", helper.display()))?;
                std::fs::File::open(&helper)
                    .with_context(|| format!("open retirement helper: {}", helper.display()))?
                    .sync_all()
                    .with_context(|| format!("sync retirement helper: {}", helper.display()))?;
            }
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect retirement helper: {}", helper.display()));
        }
    }
    Ok(helper)
}

fn validate_private_helper_executable(path: &Path) -> Result<()> {
    require_regular_file(path, "retirement helper executable")?;
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("inspect retirement helper: {}", path.display()))?;
    anyhow::ensure!(metadata.len() > 0, "retirement helper executable is empty");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        anyhow::ensure!(
            mode & 0o077 == 0 && mode & 0o100 != 0,
            "retirement helper must be private and executable: {}",
            path.display()
        );
    }
    Ok(())
}

fn repair_or_validate_private_helper_executable(path: &Path, source: &Path) -> Result<()> {
    require_regular_file(path, "retirement helper executable")?;
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("inspect retirement helper: {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o077 == 0 && mode & 0o100 == 0 {
            let helper_bytes = std::fs::read(path).with_context(|| {
                format!("read incomplete retirement helper: {}", path.display())
            })?;
            let source_bytes = std::fs::read(source).with_context(|| {
                format!("read retirement source executable: {}", source.display())
            })?;
            anyhow::ensure!(
                !helper_bytes.is_empty() && helper_bytes == source_bytes,
                "non-executable retirement helper does not match the current source binary"
            );
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("repair retirement helper mode: {}", path.display()))?;
            std::fs::File::open(path)
                .with_context(|| format!("open repaired retirement helper: {}", path.display()))?
                .sync_all()
                .with_context(|| format!("sync repaired retirement helper: {}", path.display()))?;
        }
    }
    validate_private_helper_executable(path)
}

fn retirement_helper_args(job_id: &str) -> Result<Vec<String>> {
    let job_id = normalize_ticket_id(job_id)?;
    Ok(vec![
        "apply-retirement".to_string(),
        "--job-id".to_string(),
        job_id,
    ])
}

fn sleep_until_stopped(duration: Duration, stop: &AtomicBool) -> bool {
    let mut remaining = duration;
    while !remaining.is_zero() && !stop.load(Ordering::SeqCst) {
        let step = remaining.min(Duration::from_millis(250));
        std::thread::sleep(step);
        remaining = remaining.saturating_sub(step);
    }
    stop.load(Ordering::SeqCst)
}

#[cfg(target_os = "macos")]
fn schedule_platform_helper(executable: &Path, job_id: &str, daemon_pid: u32) -> Result<()> {
    let home = std::env::var_os("HOME").context("HOME env var not set")?;
    let home = PathBuf::from(home);
    let daemon_plist = home.join("Library/LaunchAgents/com.rustory.daemon.plist");
    require_regular_file(&daemon_plist, "managed launchd daemon plist")?;
    anyhow::ensure!(
        launchd_daemon_pid()? == Some(daemon_pid),
        "remote full uninstall requires the current daemon to be the active launchd service"
    );

    let label = retirement_launchd_label(job_id);
    let plist_path = home
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist"));
    let log_path = home.join("Library/Logs/rustory-retirement.log");
    let state_home = resolved_state_home()?;
    let helper_args = retirement_helper_args(job_id)?;
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n    <string>apply-retirement</string>\n    <string>--job-id</string>\n    <string>{}</string>\n  </array>\n  <key>EnvironmentVariables</key>\n  <dict>\n    <key>{}</key><string>1</string>\n    <key>RUSTORY_DAEMON_MANAGER</key><string>launchd</string>\n    <key>XDG_STATE_HOME</key><string>{}</string>\n  </dict>\n  <key>RunAtLoad</key><true/>\n  <key>KeepAlive</key>\n  <dict><key>SuccessfulExit</key><false/></dict>\n  <key>ThrottleInterval</key><integer>15</integer>\n  <key>StandardOutPath</key><string>{}</string>\n  <key>StandardErrorPath</key><string>{}</string>\n</dict>\n</plist>\n",
        xml_escape(&label),
        xml_escape(&executable.display().to_string()),
        xml_escape(&helper_args[2]),
        RETIREMENT_HELPER_ENV,
        xml_escape(&state_home.display().to_string()),
        xml_escape(&log_path.display().to_string()),
        xml_escape(&log_path.display().to_string()),
    );
    crate::config::write_private_file(&plist_path, plist.as_bytes(), true)?;
    let uid = unsafe { libc::getuid() };
    let service = format!("gui/{uid}/{label}");
    let loaded = Command::new("launchctl")
        .arg("print")
        .arg(&service)
        .output()
        .context("inspect retirement helper launchd job")?
        .status
        .success();
    if loaded {
        let output = Command::new("launchctl")
            .arg("kickstart")
            .arg(&service)
            .output()
            .context("kick retirement helper launchd job")?;
        if !output.status.success() {
            anyhow::bail!(
                "launchctl kickstart retirement helper failed: {}",
                one_line_output(&output)
            );
        }
        return Ok(());
    }
    let output = Command::new("launchctl")
        .arg("bootstrap")
        .arg(format!("gui/{uid}"))
        .arg(&plist_path)
        .output()
        .context("launch retirement helper with launchd")?;
    if !output.status.success() {
        let concurrently_loaded = Command::new("launchctl")
            .arg("print")
            .arg(&service)
            .output()
            .is_ok_and(|output| output.status.success());
        if concurrently_loaded {
            return Ok(());
        }
        anyhow::bail!(
            "launchctl bootstrap retirement helper failed: {}",
            one_line_output(&output)
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn launchd_daemon_pid() -> Result<Option<u32>> {
    let uid = unsafe { libc::getuid() };
    let output = Command::new("launchctl")
        .arg("print")
        .arg(format!("gui/{uid}/com.rustory.daemon"))
        .output()
        .context("inspect managed launchd daemon")?;
    if !output.status.success() {
        return Ok(None);
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("pid = ") {
            return value
                .trim()
                .parse::<u32>()
                .map(Some)
                .context("parse managed launchd daemon pid");
        }
    }
    Ok(None)
}

#[cfg(target_os = "linux")]
fn schedule_platform_helper(executable: &Path, job_id: &str, daemon_pid: u32) -> Result<()> {
    let Some(_daemon_unit) = active_systemd_daemon_unit(daemon_pid)? else {
        anyhow::bail!(
            "remote full uninstall requires an active managed systemd user service; background daemons support revoke-only retirement"
        );
    };
    let helper_args = retirement_helper_args(job_id)?;
    let unit = retirement_systemd_unit_name(job_id);
    let unit_path = retirement_systemd_unit_path(job_id)?;
    let exec_start = std::iter::once(executable.display().to_string())
        .chain(helper_args.iter().cloned())
        .map(|argument| systemd_quote_exec_arg(&argument))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    let state_environment = systemd_quote_environment(
        "XDG_STATE_HOME",
        &resolved_state_home()?.display().to_string(),
    )?;
    let unit_contents = format!(
        "[Unit]\nDescription=Rustory device retirement {job_id}\nAfter=network-online.target\nWants=network-online.target\nStartLimitIntervalSec=0\n\n[Service]\nType=exec\nExecStart={exec_start}\nEnvironment={RETIREMENT_HELPER_ENV}=1\nEnvironment=RUSTORY_DAEMON_MANAGER=systemd-user\nEnvironment={state_environment}\nRestart=on-failure\nRestartSec=15\n\n[Install]\nWantedBy=default.target\n"
    );
    crate::config::write_private_file(&unit_path, unit_contents.as_bytes(), true)?;
    run_systemctl_user(["daemon-reload"], "reload retirement helper unit")?;
    run_systemctl_user(
        ["enable", unit.as_str()],
        "enable retirement helper recovery unit",
    )?;
    if systemd_unit_is_active(&unit)? {
        return Ok(());
    }
    run_systemctl_user(
        ["reset-failed", unit.as_str()],
        "reset retirement helper failure state",
    )?;
    run_systemctl_user(
        ["start", unit.as_str()],
        "start retirement helper recovery unit",
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn retirement_systemd_unit_name(job_id: &str) -> String {
    format!("rustory-retire-{}.service", job_id.replace('-', ""))
}

#[cfg(target_os = "linux")]
fn retirement_systemd_unit_path(job_id: &str) -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME env var not set")?);
    anyhow::ensure!(
        home.is_absolute(),
        "HOME must be absolute for systemd user units"
    );
    Ok(home
        .join(".config/systemd/user")
        .join(retirement_systemd_unit_name(job_id)))
}

#[cfg(target_os = "linux")]
fn systemd_quote_exec_arg(value: &str) -> Result<String> {
    anyhow::ensure!(
        !value.is_empty() && !value.chars().any(char::is_control),
        "systemd retirement argument is empty or contains control characters"
    );
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$");
    Ok(format!("\"{escaped}\""))
}

#[cfg(target_os = "linux")]
fn systemd_quote_environment(name: &str, value: &str) -> Result<String> {
    anyhow::ensure!(
        !value.is_empty() && !value.chars().any(char::is_control),
        "systemd retirement environment contains an empty or unsafe value"
    );
    let escaped = format!("{name}={value}")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    Ok(format!("\"{escaped}\""))
}

#[cfg(target_os = "linux")]
fn run_systemctl_user<const N: usize>(args: [&str; N], label: &str) -> Result<()> {
    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .with_context(|| label.to_string())?;
    anyhow::ensure!(
        output.status.success(),
        "{label} failed: {}",
        one_line_output(&output)
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn active_systemd_daemon_unit(daemon_pid: u32) -> Result<Option<&'static str>> {
    for unit in ["rustory.service", "rustory-daemon.service"] {
        if !systemd_unit_is_active(unit)? {
            continue;
        }
        let output = Command::new("systemctl")
            .args(["--user", "show", "--property=MainPID", "--value", unit])
            .output()
            .with_context(|| format!("inspect systemd daemon pid for {unit}"))?;
        if output.status.success()
            && String::from_utf8_lossy(&output.stdout).trim() == daemon_pid.to_string()
        {
            return Ok(Some(unit));
        }
    }
    Ok(None)
}

#[cfg(target_os = "linux")]
fn systemd_unit_is_active(unit: &str) -> Result<bool> {
    let output = Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", unit])
        .output()
        .with_context(|| format!("inspect systemd user unit {unit}"))?;
    Ok(output.status.success())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn schedule_platform_helper(_executable: &Path, _job_id: &str, _daemon_pid: u32) -> Result<()> {
    anyhow::bail!("remote retirement helper is unsupported on this platform")
}

fn require_regular_file(path: &Path, label: &str) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("inspect {label}: {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "{label} must be a regular non-symlink file: {}",
        path.display()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn retirement_launchd_label(job_id: &str) -> String {
    format!("com.rustory.retire.{}", job_id.replace('-', ""))
}

#[cfg(target_os = "macos")]
fn cleanup_retirement_helper_artifact(job_id: &str) -> Result<()> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME env var not set")?);
    let path = home
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", retirement_launchd_label(job_id)));
    remove_regular_file_if_present(&path)?;
    remove_regular_file_if_present(&home.join("Library/Logs/rustory-retirement.log"))
}

#[cfg(target_os = "linux")]
fn cleanup_retirement_helper_artifact(job_id: &str) -> Result<()> {
    let unit = retirement_systemd_unit_name(job_id);
    let unit_path = retirement_systemd_unit_path(job_id)?;
    match std::fs::symlink_metadata(&unit_path) {
        Ok(_) => {
            require_regular_file(&unit_path, "retirement systemd user unit")?;
            run_systemctl_user(
                ["disable", unit.as_str()],
                "disable retirement helper recovery unit",
            )?;
            remove_regular_file_if_present(&unit_path)?;
            if let Err(error) = run_systemctl_user(["daemon-reload"], "reload systemd user units") {
                eprintln!(
                    "warn: retirement unit was disabled and removed but systemd daemon-reload failed: {error:#}"
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!("inspect retirement systemd unit: {}", unit_path.display())
            });
        }
    }
    remove_regular_file_if_present(&rustory_state_dir()?.join("retirement.log"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn cleanup_retirement_helper_artifact(_job_id: &str) -> Result<()> {
    Ok(())
}

fn remove_regular_file_if_present(path: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect helper artifact: {}", path.display()));
        }
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "refusing to remove unsafe helper artifact: {}",
        path.display()
    );
    std::fs::remove_file(path)
        .with_context(|| format!("remove helper artifact: {}", path.display()))?;
    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(path)
            .with_context(|| format!("open directory for sync: {}", path.display()))?
            .sync_all()
            .with_context(|| format!("sync directory: {}", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn one_line_output(output: &std::process::Output) -> String {
    format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}

pub fn sign_device_action(
    identity: &crate::libp2p::identity::Keypair,
    action: &str,
    payload: &[u8],
) -> Result<DeviceProof> {
    sign_device_action_at(
        identity,
        action,
        payload,
        time::OffsetDateTime::now_utc().unix_timestamp(),
        uuid::Uuid::new_v4().to_string(),
    )
}

fn sign_device_action_at(
    identity: &crate::libp2p::identity::Keypair,
    action: &str,
    payload: &[u8],
    issued_at_unix: i64,
    nonce: String,
) -> Result<DeviceProof> {
    validate_action(action)?;
    validate_nonce(&nonce)?;

    let public = identity.public();
    let peer_id = public.to_peer_id().to_string();
    let canonical = canonical_device_action(action, &peer_id, issued_at_unix, &nonce, payload);
    let signature = identity
        .sign(&canonical)
        .context("sign device action with identity key")?;

    Ok(DeviceProof {
        peer_id,
        issued_at_unix,
        nonce,
        public_key: public.encode_protobuf(),
        signature,
    })
}

pub fn verify_device_action(
    proof: &DeviceProof,
    action: &str,
    payload: &[u8],
    expected_peer_id: &str,
    now_unix: i64,
) -> Result<crate::libp2p::identity::PublicKey> {
    validate_action(action)?;
    validate_nonce(&proof.nonce)?;
    if proof.peer_id != expected_peer_id {
        anyhow::bail!(
            "device proof peer_id mismatch: got={} want={expected_peer_id}",
            proof.peer_id
        );
    }
    if proof.public_key.is_empty()
        || proof.public_key.len() > DEVICE_PROOF_MAX_PUBLIC_KEY_BYTES
        || proof.signature.is_empty()
        || proof.signature.len() > DEVICE_PROOF_MAX_SIGNATURE_BYTES
    {
        anyhow::bail!("device proof key or signature size is invalid");
    }
    if now_unix.abs_diff(proof.issued_at_unix) > DEVICE_PROOF_MAX_SKEW_SEC as u64 {
        anyhow::bail!("device proof timestamp is outside the allowed clock skew");
    }

    let public = crate::libp2p::identity::PublicKey::try_decode_protobuf(&proof.public_key)
        .context("decode device proof public key")?;
    let derived_peer_id = public.to_peer_id().to_string();
    if derived_peer_id != expected_peer_id {
        anyhow::bail!(
            "device proof public key does not match peer_id: got={derived_peer_id} want={expected_peer_id}"
        );
    }

    let canonical = canonical_device_action(
        action,
        expected_peer_id,
        proof.issued_at_unix,
        &proof.nonce,
        payload,
    );
    if !public.verify(&canonical, &proof.signature) {
        anyhow::bail!("device proof signature verification failed");
    }
    Ok(public)
}

fn canonical_device_action(
    action: &str,
    peer_id: &str,
    issued_at_unix: i64,
    nonce: &str,
    payload: &[u8],
) -> Vec<u8> {
    let digest = Sha256::digest(payload);
    format!(
        "rustory-device-proof-v1\n{action}\n{peer_id}\n{issued_at_unix}\n{nonce}\n{}",
        hex_lower(&digest)
    )
    .into_bytes()
}

fn validate_action(action: &str) -> Result<()> {
    if action.is_empty()
        || action.len() > 64
        || !action
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte == b'-')
    {
        anyhow::bail!("invalid device proof action");
    }
    Ok(())
}

fn validate_nonce(nonce: &str) -> Result<()> {
    if nonce.is_empty()
        || nonce.len() > DEVICE_PROOF_MAX_NONCE_BYTES
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        anyhow::bail!("invalid device proof nonce");
    }
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_device_proof_binds_action_payload_and_peer_id() {
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let proof = sign_device_action_at(
            &identity,
            "register",
            br#"{"device":"node0"}"#,
            now,
            "nonce-1".to_string(),
        )
        .unwrap();

        verify_device_action(
            &proof,
            "register",
            br#"{"device":"node0"}"#,
            &proof.peer_id,
            now,
        )
        .unwrap();
        assert!(
            verify_device_action(
                &proof,
                "register",
                br#"{"device":"node1"}"#,
                &proof.peer_id,
                now,
            )
            .is_err()
        );
        assert!(
            verify_device_action(
                &proof,
                "retirement_poll",
                br#"{"device":"node0"}"#,
                &proof.peer_id,
                now,
            )
            .is_err()
        );
    }

    #[test]
    fn tracker_device_proof_rejects_wrong_key_and_stale_timestamp() {
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let other = crate::libp2p::identity::Keypair::generate_ed25519();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let proof = sign_device_action_at(
            &identity,
            "register",
            b"payload",
            now,
            "nonce-2".to_string(),
        )
        .unwrap();

        assert!(
            verify_device_action(
                &proof,
                "register",
                b"payload",
                &other.public().to_peer_id().to_string(),
                now,
            )
            .is_err()
        );
        assert!(
            verify_device_action(
                &proof,
                "register",
                b"payload",
                &proof.peer_id,
                now + DEVICE_PROOF_MAX_SKEW_SEC + 1,
            )
            .is_err()
        );
    }

    #[test]
    fn retirement_tracker_url_rejects_loopback_prefix_spoofing_and_plaintext_remote() {
        validate_retirement_tracker_url("https://tracker.example").unwrap();
        validate_retirement_tracker_url("http://localhost:8850").unwrap();
        validate_retirement_tracker_url("http://127.0.0.1:8850").unwrap();
        validate_retirement_tracker_url("http://[::1]:8850").unwrap();

        for unsafe_url in [
            "http://localhost.evil.example",
            "http://127.0.0.1.evil.example",
            "http://[::1].evil.example",
            "http://tracker.example",
            "http://user@localhost:8850",
        ] {
            assert!(
                validate_retirement_tracker_url(unsafe_url).is_err(),
                "unexpectedly accepted {unsafe_url}"
            );
        }
    }

    #[test]
    fn retirement_helper_accepts_only_fixed_canonical_ticket_arguments() {
        let ticket_id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            retirement_helper_args(&ticket_id).unwrap(),
            vec!["apply-retirement", "--job-id", ticket_id.as_str()]
        );
        assert!(retirement_helper_args("../../tmp/job").is_err());
        assert!(retirement_helper_args(&ticket_id.to_ascii_uppercase()).is_err());
    }

    #[test]
    fn retirement_ticket_is_bound_to_target_identity_and_cleanup_enum() {
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let other = crate::libp2p::identity::Keypair::generate_ed25519();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut ticket = RetirementTicket {
            ticket_id: uuid::Uuid::new_v4().to_string(),
            peer_id: identity.public().to_peer_id().to_string(),
            device_id: Some("node0".to_string()),
            user_id: Some("u1".to_string()),
            cleanup: RetirementCleanup::FullUninstall,
            issued_at_unix: now,
            status: RetirementStatus::Pending,
            status_updated_at_unix: now,
            status_detail: None,
        };
        validate_retirement_ticket(&ticket, &identity).unwrap();
        assert!(validate_retirement_ticket(&ticket, &other).is_err());
        ticket.cleanup = RetirementCleanup::RevokeOnly;
        assert!(validate_retirement_ticket(&ticket, &identity).is_err());
    }

    #[test]
    fn revoke_only_ticket_is_not_terminal_for_upgrade_monitoring() {
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let mut ticket = RetirementTicket {
            ticket_id: uuid::Uuid::new_v4().to_string(),
            peer_id: identity.public().to_peer_id().to_string(),
            device_id: Some("device".to_string()),
            user_id: Some("user".to_string()),
            cleanup: RetirementCleanup::RevokeOnly,
            status: RetirementStatus::Completed,
            issued_at_unix: now,
            status_updated_at_unix: now,
            status_detail: None,
        };

        assert!(!retirement_ticket_is_terminal_for_monitor(&ticket));
        ticket.cleanup = RetirementCleanup::FullUninstall;
        assert!(retirement_ticket_is_terminal_for_monitor(&ticket));
    }

    #[test]
    fn completion_capability_is_random_fixed_width_and_hashable() {
        let (first, first_hash) = generate_completion_capability().unwrap();
        let (second, second_hash) = generate_completion_capability().unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(first_hash.len(), 64);
        assert_eq!(completion_capability_hash(&first).unwrap(), first_hash);
        assert_ne!(first, second);
        assert_ne!(first_hash, second_hash);
        assert!(completion_capability_hash(&first.to_ascii_uppercase()).is_err());
    }

    #[test]
    fn stable_retirement_request_keeps_first_startup_plan_across_recovery() {
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let ticket = RetirementTicket {
            ticket_id: uuid::Uuid::new_v4().to_string(),
            peer_id: identity.public().to_peer_id().to_string(),
            device_id: Some("node0".to_string()),
            user_id: Some("user0".to_string()),
            cleanup: RetirementCleanup::FullUninstall,
            issued_at_unix: now,
            status: RetirementStatus::Pending,
            status_updated_at_unix: now,
            status_detail: None,
        };
        let requested = RetirementJob {
            version: RETIREMENT_JOB_VERSION,
            tracker_url: "https://tracker.example".to_string(),
            install_path: PathBuf::from("/opt/rustory/rr"),
            identity_key_path: PathBuf::from("/home/user/.config/rustory/identity.key"),
            ticket,
            cleanup_plan: Some(RetirementCleanupPlan {
                install_path: PathBuf::from("/opt/rustory/rr"),
                config_path: PathBuf::from("/home/user/.config/rustory/config.toml"),
                db_path: PathBuf::from("/home/user/.local/share/rustory/history.db"),
                config_key_paths: Vec::new(),
                state_marker_paths: Vec::new(),
                extra_rc_files: Vec::new(),
                local_peer_id: identity.public().to_peer_id().to_string(),
            }),
            completion_capability: None,
            running_accepted: false,
            cleanup_completed: false,
        };
        validate_retirement_ticket_for_storage(&requested).unwrap();
        let (capability, _) = generate_completion_capability().unwrap();
        let mut existing = requested.clone();
        existing.completion_capability = Some(capability);
        existing.running_accepted = true;
        existing.cleanup_completed = true;
        existing.ticket.status = RetirementStatus::Running;
        existing.ticket.status_updated_at_unix += 10;
        existing.ticket.status_detail = Some("progress".to_string());
        assert!(same_retirement_request(&existing, &requested));

        let mut config_drifted_request = requested.clone();
        config_drifted_request
            .cleanup_plan
            .as_mut()
            .unwrap()
            .db_path = PathBuf::from("/tmp/new-config-path.db");
        assert!(same_retirement_request(&existing, &config_drifted_request));

        existing.install_path = PathBuf::from("/tmp/other-rr");
        assert!(!same_retirement_request(&existing, &requested));
    }

    #[test]
    fn bounded_status_detail_preserves_utf8_boundaries_and_removes_controls() {
        let detail = format!("line\n{}", "한".repeat(200));
        let bounded = bounded_status_detail(&detail);
        assert!(bounded.len() <= 240);
        assert!(!bounded.contains('\n'));
        assert!(std::str::from_utf8(bounded.as_bytes()).is_ok());
    }

    #[test]
    fn retirement_cleanup_removes_only_an_empty_state_directory() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("rustory");
        std::fs::create_dir(&state_dir).unwrap();
        remove_empty_retirement_state_dir(&state_dir).unwrap();
        assert!(!state_dir.exists());

        std::fs::create_dir(&state_dir).unwrap();
        std::fs::write(state_dir.join("keep-me"), b"user state").unwrap();
        remove_empty_retirement_state_dir(&state_dir).unwrap();
        assert!(state_dir.join("keep-me").exists());
    }
}
