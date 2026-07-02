use std::path::Path;
use std::process::{Child, Command as ProcessCommand, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::sync_status::{SyncStatusTrackerReport, build_tracker_status_report};

#[derive(Debug, Clone)]
pub(crate) struct DaemonArgs {
    pub(crate) listen: String,
    pub(crate) identity_key: Option<String>,
    pub(crate) swarm_key: Option<String>,
    pub(crate) relay: Option<String>,
    pub(crate) trackers: Vec<String>,
    pub(crate) tracker_token: Option<String>,
    pub(crate) limit: usize,
    pub(crate) pull_only: bool,
    pub(crate) interval_sec: u64,
    pub(crate) start_jitter_sec: Option<u64>,
    pub(crate) sync_start_delay_sec: u64,
    pub(crate) max_peers_per_tick: usize,
    pub(crate) preflight: bool,
    pub(crate) req_attempts: Option<u64>,
    pub(crate) req_timeout_base_sec: Option<u64>,
    pub(crate) req_timeout_cap_sec: Option<u64>,
    pub(crate) req_backoff_base_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DaemonChildSpecs {
    pub(crate) serve_args: Vec<String>,
    pub(crate) sync_args: Vec<String>,
    pub(crate) tracker_token_env: Option<String>,
}

pub(crate) fn run_daemon_preflight(trackers: &[String], tracker_token: Option<&str>) -> Result<()> {
    eprintln!("daemon preflight: ping {} tracker(s)", trackers.len());
    let statuses = build_tracker_status_report(trackers, tracker_token);
    for status in &statuses {
        if status.reachable {
            let latency_ms = status
                .latency_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string());
            eprintln!(
                "daemon preflight: tracker {} ok latency_ms={latency_ms}",
                status.base_url
            );
        } else {
            let error = status.error.as_deref().unwrap_or("unknown error");
            eprintln!(
                "daemon preflight: tracker {} failed: {error}",
                status.base_url
            );
        }
    }
    validate_daemon_preflight_statuses(&statuses)
}

pub(crate) fn validate_daemon_preflight_statuses(
    statuses: &[SyncStatusTrackerReport],
) -> Result<()> {
    let failures = statuses
        .iter()
        .filter(|status| !status.reachable)
        .collect::<Vec<_>>();
    if failures.is_empty() {
        return Ok(());
    }

    let details = failures
        .iter()
        .map(|status| {
            let error = status.error.as_deref().unwrap_or("unknown error");
            format!("{} ({error})", status.base_url)
        })
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!(
        "daemon preflight failed: {}/{} tracker ping(s) failed: {details}",
        failures.len(),
        statuses.len()
    );
}

pub(crate) fn build_daemon_child_specs(db_path: &str, args: &DaemonArgs) -> DaemonChildSpecs {
    let mut serve_args = vec![
        "--db-path".to_string(),
        db_path.to_string(),
        "p2p-serve".to_string(),
        "--listen".to_string(),
        args.listen.clone(),
    ];
    push_optional_arg(
        &mut serve_args,
        "--identity-key",
        args.identity_key.as_deref(),
    );
    push_optional_arg(&mut serve_args, "--swarm-key", args.swarm_key.as_deref());
    push_optional_arg(&mut serve_args, "--relay", args.relay.as_deref());
    push_trackers_arg(&mut serve_args, &args.trackers);

    let mut sync_args = vec![
        "--db-path".to_string(),
        db_path.to_string(),
        "p2p-sync".to_string(),
        "--watch".to_string(),
        "--limit".to_string(),
        args.limit.to_string(),
        "--interval-sec".to_string(),
        args.interval_sec.max(1).to_string(),
        "--max-peers-per-tick".to_string(),
        args.max_peers_per_tick.to_string(),
    ];
    if !args.pull_only {
        sync_args.push("--push".to_string());
    }
    if let Some(v) = args.start_jitter_sec {
        sync_args.push("--start-jitter-sec".to_string());
        sync_args.push(v.to_string());
    }
    push_optional_arg(&mut sync_args, "--swarm-key", args.swarm_key.as_deref());
    push_optional_arg(
        &mut sync_args,
        "--identity-key",
        args.identity_key.as_deref(),
    );
    push_optional_arg(&mut sync_args, "--relay", args.relay.as_deref());
    push_trackers_arg(&mut sync_args, &args.trackers);
    push_optional_u64_arg(&mut sync_args, "--req-attempts", args.req_attempts);
    push_optional_u64_arg(
        &mut sync_args,
        "--req-timeout-base-sec",
        args.req_timeout_base_sec,
    );
    push_optional_u64_arg(
        &mut sync_args,
        "--req-timeout-cap-sec",
        args.req_timeout_cap_sec,
    );
    push_optional_u64_arg(
        &mut sync_args,
        "--req-backoff-base-ms",
        args.req_backoff_base_ms,
    );

    DaemonChildSpecs {
        serve_args,
        sync_args,
        tracker_token_env: args.tracker_token.clone(),
    }
}

fn push_optional_arg(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        args.push(flag.to_string());
        args.push(value.to_string());
    }
}

fn push_optional_u64_arg(args: &mut Vec<String>, flag: &str, value: Option<u64>) {
    if let Some(value) = value {
        args.push(flag.to_string());
        args.push(value.to_string());
    }
}

fn push_trackers_arg(args: &mut Vec<String>, trackers: &[String]) {
    if trackers.is_empty() {
        return;
    }
    args.push("--trackers".to_string());
    args.push(trackers.join(","));
}

pub(crate) fn spawn_daemon_child(
    label: &str,
    exe: &Path,
    args: &[String],
    tracker_token_env: Option<&str>,
) -> Result<Child> {
    eprintln!("daemon: spawn {label}: {}", redacted_command(exe, args));
    let mut cmd = ProcessCommand::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(token) = tracker_token_env {
        cmd.env("RUSTORY_TRACKER_TOKEN", token);
    }
    cmd.spawn()
        .with_context(|| format!("spawn daemon child: {label}"))
}

fn redacted_command(exe: &Path, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(exe.display().to_string());
    parts.extend(args.iter().cloned());
    parts.join(" ")
}

pub(crate) fn supervise_daemon_children(
    serve: &mut Child,
    sync: &mut Child,
    stop: &AtomicBool,
) -> Result<()> {
    loop {
        if stop.load(Ordering::SeqCst) {
            eprintln!("daemon: shutdown requested");
            terminate_daemon_child("p2p-sync", sync)?;
            terminate_daemon_child("p2p-serve", serve)?;
            return Ok(());
        }

        if let Some(status) = serve.try_wait().context("poll p2p-serve child")? {
            terminate_daemon_child("p2p-sync", sync)?;
            anyhow::bail!("daemon child p2p-serve exited: {status}");
        }

        if let Some(status) = sync.try_wait().context("poll p2p-sync child")? {
            terminate_daemon_child("p2p-serve", serve)?;
            anyhow::bail!("daemon child p2p-sync exited: {status}");
        }

        std::thread::sleep(Duration::from_millis(500));
    }
}

pub(crate) fn sleep_with_stop(duration: Duration, stop: &AtomicBool) {
    let deadline = Instant::now() + duration;
    while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub(crate) fn terminate_daemon_child(label: &str, child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }

    send_child_terminate(child).with_context(|| format!("terminate daemon child: {label}"))?;
    if let Some(status) = wait_child_timeout(child, Duration::from_secs(5))? {
        eprintln!("daemon: {label} stopped: {status}");
        return Ok(());
    }

    eprintln!("warn: daemon child {label} did not stop after SIGTERM; killing");
    child
        .kill()
        .with_context(|| format!("kill daemon child: {label}"))?;
    let status = child
        .wait()
        .with_context(|| format!("wait killed daemon child: {label}"))?;
    eprintln!("daemon: {label} killed: {status}");
    Ok(())
}

fn wait_child_timeout(child: &mut Child, timeout: Duration) -> Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(None)
}

fn send_child_terminate(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        Err(err)
    }

    #[cfg(not(unix))]
    {
        child.kill()
    }
}
