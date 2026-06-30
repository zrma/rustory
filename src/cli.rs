use crate::libp2p;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use multiaddr::Protocol;
use rand::RngExt;

use crate::{
    config, hishtory_cleanup, history_import, hook, p2p, search, storage, tracker, transport,
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const DEFAULT_ASYNC_UPLOAD_INTERVAL_SEC: u64 = 15;
const DEFAULT_ASYNC_UPLOAD_LIMIT: usize = 200;
const DEFAULT_ASYNC_UPLOAD_MARKER_PATH: &str = "~/.config/rustory/async-upload.last";
const DEFAULT_AUTO_PRUNE_DAYS: u64 = 180;
const DEFAULT_AUTO_PRUNE_INTERVAL_SEC: u64 = 86_400;
const DEFAULT_AUTO_PRUNE_KEEP_RECENT: usize = 0;
const DEFAULT_AUTO_PRUNE_MARKER_PATH: &str = "~/.config/rustory/auto-prune.last";
const DEFAULT_HOOK_SEARCH_LIMIT: usize = 100_000;
const DEFAULT_RECORD_IGNORE_REGEX: &str = r"(?i)(password|passwd|token|secret|authorization:|bearer |api[_-]?key|access[_-]?key|private[_-]?key)";

#[derive(Parser)]
#[command(name = "rr", version = crate::build_info::VERSION_DISPLAY, about = "Rustory CLI")]
pub struct App {
    #[arg(
        long,
        global = true,
        help = "Path to the local SQLite history database"
    )]
    db_path: Option<String>,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Serve the debug HTTP sync API")]
    Serve {
        #[arg(
            long,
            default_value = "127.0.0.1:8844",
            help = "TCP bind address for the debug HTTP server"
        )]
        bind: String,

        #[arg(long, help = "Bearer token required by HTTP sync clients")]
        token: Option<String>,

        #[arg(
            long,
            help = "Allow serving the HTTP sync API without a token on non-loopback bind addresses"
        )]
        allow_unauthenticated: bool,
    },
    #[command(about = "Sync with HTTP peers")]
    Sync {
        #[arg(
            long,
            value_delimiter = ',',
            help = "Comma-separated HTTP peer base URLs to sync with"
        )]
        peers: Vec<String>,

        #[arg(long, help = "Push local entries to peers after pulling")]
        push: bool,

        #[arg(long, help = "Bearer token sent to HTTP sync peers")]
        token: Option<String>,
    },
    #[command(about = "Serve this device as a P2P peer")]
    P2pServe {
        #[arg(
            long,
            default_value = "/ip4/0.0.0.0/tcp/0",
            help = "libp2p multiaddr to listen on"
        )]
        listen: String,

        #[arg(long, help = "Path to this device's persistent P2P identity key")]
        identity_key: Option<String>,

        #[arg(long, help = "Path to the shared private swarm key")]
        swarm_key: Option<String>,

        #[arg(long, help = "Relay multiaddr used for relay reservation")]
        relay: Option<String>,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Comma-separated tracker base URLs for peer discovery"
        )]
        trackers: Vec<String>,

        #[arg(long, help = "Bearer token sent to tracker endpoints")]
        tracker_token: Option<String>,
    },
    #[command(about = "Sync with P2P peers, trackers, or cached peers")]
    P2pSync {
        #[arg(
            long,
            value_delimiter = ',',
            help = "Comma-separated peer multiaddrs to dial directly"
        )]
        peers: Vec<String>,

        #[arg(
            long,
            default_value_t = 1000,
            help = "Maximum entries per pull or push batch"
        )]
        limit: usize,

        #[arg(long, help = "Push local entries after pulling from peers")]
        push: bool,

        #[arg(long, help = "Run sync repeatedly until interrupted")]
        watch: bool,

        #[arg(
            long,
            default_value_t = 60,
            help = "Seconds between watch-mode sync attempts"
        )]
        interval_sec: u64,

        #[arg(long, help = "Random initial delay upper bound for watch mode")]
        start_jitter_sec: Option<u64>,

        #[arg(
            long,
            default_value_t = 0,
            help = "Maximum tracker-discovered peers to sync per watch tick; 0 means all peers"
        )]
        max_peers_per_tick: usize,

        #[arg(long, help = "Request retry attempts per peer operation")]
        req_attempts: Option<u64>,

        #[arg(long, help = "Initial request timeout in seconds before backoff")]
        req_timeout_base_sec: Option<u64>,

        #[arg(long, help = "Maximum request timeout in seconds after backoff")]
        req_timeout_cap_sec: Option<u64>,

        #[arg(long, help = "Base retry backoff in milliseconds")]
        req_backoff_base_ms: Option<u64>,

        #[arg(long, help = "Path to this device's persistent P2P identity key")]
        identity_key: Option<String>,

        #[arg(long, help = "Path to the shared private swarm key")]
        swarm_key: Option<String>,

        #[arg(long, help = "Relay multiaddr preferred for tracker-discovered peers")]
        relay: Option<String>,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Comma-separated tracker base URLs for peer discovery"
        )]
        trackers: Vec<String>,

        #[arg(long, help = "Bearer token sent to tracker endpoints")]
        tracker_token: Option<String>,
    },
    #[command(about = "Run p2p-serve plus p2p-sync watch as one supervised process")]
    Daemon {
        #[arg(
            long,
            default_value = "/ip4/0.0.0.0/tcp/0",
            help = "libp2p multiaddr for the embedded p2p-serve listener"
        )]
        listen: String,

        #[arg(long, help = "Path to this device's persistent P2P identity key")]
        identity_key: Option<String>,

        #[arg(long, help = "Path to the shared private swarm key")]
        swarm_key: Option<String>,

        #[arg(long, help = "Relay multiaddr used for reservation and sync")]
        relay: Option<String>,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Comma-separated tracker base URLs for peer discovery"
        )]
        trackers: Vec<String>,

        #[arg(long, help = "Bearer token sent to tracker endpoints")]
        tracker_token: Option<String>,

        #[arg(
            long,
            default_value_t = 1000,
            help = "Maximum entries per pull or push batch"
        )]
        limit: usize,

        #[arg(long = "pull-only", help = "Disable pushing local entries to peers")]
        pull_only: bool,

        #[arg(
            long,
            default_value_t = 60,
            help = "Seconds between sync watch attempts"
        )]
        interval_sec: u64,

        #[arg(long, help = "Random initial delay upper bound for sync watch mode")]
        start_jitter_sec: Option<u64>,

        #[arg(
            long,
            default_value_t = 2,
            help = "Seconds to wait after p2p-serve starts before starting sync watch"
        )]
        sync_start_delay_sec: u64,

        #[arg(
            long,
            default_value_t = 0,
            help = "Maximum tracker-discovered peers to sync per daemon tick; 0 means all peers"
        )]
        max_peers_per_tick: usize,

        #[arg(
            long,
            default_value_t = false,
            help = "Ping configured trackers before spawning daemon children"
        )]
        preflight: bool,

        #[arg(long, help = "Request retry attempts per peer operation")]
        req_attempts: Option<u64>,

        #[arg(long, help = "Initial request timeout in seconds before backoff")]
        req_timeout_base_sec: Option<u64>,

        #[arg(long, help = "Maximum request timeout in seconds after backoff")]
        req_timeout_cap_sec: Option<u64>,

        #[arg(long, help = "Base retry backoff in milliseconds")]
        req_backoff_base_ms: Option<u64>,
    },
    #[command(about = "Create or inspect the shared P2P swarm key")]
    SwarmKey {
        #[arg(long, help = "Path to the shared private swarm key")]
        swarm_key: Option<String>,
    },
    #[command(about = "Record one shell command into the local history store")]
    Record {
        #[arg(long, help = "Shell command line to record")]
        cmd: String,

        #[arg(long, help = "Working directory where the command ran")]
        cwd: Option<String>,

        #[arg(long, default_value_t = 0, help = "Command exit status")]
        exit_code: i32,

        #[arg(long, default_value_t = 0, help = "Command runtime in milliseconds")]
        duration_ms: i64,

        #[arg(long, help = "Shell name that produced the command")]
        shell: Option<String>,

        #[arg(long, help = "Hostname where the command ran")]
        hostname: Option<String>,

        #[arg(long, help = "Logical Rustory user id for this entry")]
        user_id: Option<String>,

        #[arg(long, help = "Device id for this entry")]
        device_id: Option<String>,

        #[arg(long, default_value_t = false, help = "Print the inserted entry id")]
        print_id: bool,
    },
    #[command(about = "Search local history with the inline TUI")]
    Search {
        #[arg(long, help = "Maximum recent entries to offer to the search TUI")]
        limit: Option<usize>,
    },
    #[command(about = "Delete old local history entries")]
    Prune {
        #[arg(long, help = "Delete entries older than this many days")]
        older_than_days: u64,

        #[arg(long, help = "Always keep at least this many recent entries")]
        keep_recent: Option<usize>,

        #[arg(
            long,
            default_value_t = false,
            help = "Report matching rows without deleting"
        )]
        dry_run: bool,
    },
    #[command(about = "Delete selected local history entries")]
    Delete {
        #[arg(
            long,
            value_delimiter = ',',
            help = "Entry id to delete; may be passed multiple times or comma-separated"
        )]
        entry_id: Vec<String>,

        #[arg(long, help = "Delete entries whose command matches this regex")]
        cmd_regex: Option<String>,

        #[arg(
            long,
            default_value_t = false,
            help = "Report matching rows without deleting"
        )]
        dry_run: bool,

        #[arg(
            long,
            default_value_t = false,
            help = "Required for non-dry-run deletion"
        )]
        yes: bool,

        #[arg(
            long,
            default_value_t = false,
            help = "Run SQLite WAL checkpoint and VACUUM after deletion"
        )]
        vacuum: bool,
    },
    #[command(about = "Show local pull and push cursor status")]
    SyncStatus {
        #[arg(long, help = "Show status for one peer id only")]
        peer: Option<String>,

        #[arg(long, default_value_t = false, help = "Print status as pretty JSON")]
        json: bool,

        #[arg(
            long = "with-tracker",
            default_value_t = false,
            help = "Ping configured trackers and include reachability"
        )]
        with_tracker: bool,

        #[arg(
            long,
            default_value_t = false,
            help = "Continuously redraw sync status"
        )]
        watch: bool,

        #[arg(
            long,
            default_value_t = 2,
            help = "Seconds between sync-status watch refreshes"
        )]
        interval_sec: u64,
    },
    #[command(about = "Print Rustory version and build revision")]
    Version {
        #[arg(
            long,
            default_value_t = false,
            help = "Print version info as pretty JSON"
        )]
        json: bool,
    },
    #[command(about = "Self-update the rr binary from release assets")]
    Update {
        #[arg(
            long,
            default_value = "latest",
            help = "Release version to install: latest or a tag such as v1.0.2"
        )]
        version: String,

        #[arg(
            long,
            default_value = crate::self_update::DEFAULT_RELEASE_REPO,
            help = "GitHub repository that publishes Rustory release assets"
        )]
        repo: String,

        #[arg(
            long,
            help = "Override release asset base URL; downloads <base>/rr-<target>"
        )]
        asset_base_url: Option<String>,

        #[arg(long, help = "Override exact release asset URL")]
        asset_url: Option<String>,

        #[arg(
            long,
            help = "Override SHA-256 checksum URL; defaults to <asset-url>.sha256"
        )]
        checksum_url: Option<String>,

        #[arg(
            long,
            help = "Expected SHA-256 hex; when set, skip checksum URL download"
        )]
        sha256: Option<String>,

        #[arg(
            long,
            help = "Install path override; defaults to the current rr executable"
        )]
        install_path: Option<String>,

        #[arg(
            long,
            default_value_t = false,
            help = "Print the update plan without downloading or replacing rr"
        )]
        dry_run: bool,

        #[arg(
            long,
            default_value_t = false,
            help = "Do not restart a managed Rustory daemon after replacing rr"
        )]
        no_restart_daemon: bool,
    },
    #[command(about = "Print a bash or zsh shell hook")]
    Hook {
        #[arg(
            long,
            default_value = "zsh",
            help = "Shell hook to render: bash or zsh"
        )]
        shell: String,
    },
    #[command(about = "Run the lightweight P2P peer tracker")]
    TrackerServe {
        #[arg(
            long,
            default_value = "0.0.0.0:8850",
            help = "TCP bind address for the tracker HTTP server"
        )]
        bind: String,

        #[arg(
            long,
            default_value_t = 60,
            help = "Seconds before registered peers expire"
        )]
        ttl_sec: u64,

        #[arg(long, help = "Bearer token required by tracker clients")]
        token: Option<String>,

        #[arg(
            long,
            help = "Allow serving the tracker without a token on non-loopback bind addresses"
        )]
        allow_unauthenticated: bool,
    },
    #[command(about = "Run the P2P relay service")]
    RelayServe {
        #[arg(
            long,
            default_value = "/ip4/0.0.0.0/tcp/4001",
            help = "libp2p multiaddr to listen on"
        )]
        listen: String,

        #[arg(long, help = "Path to the relay's persistent identity key")]
        identity_key: Option<String>,

        #[arg(long, help = "Path to the shared private swarm key")]
        swarm_key: Option<String>,

        #[arg(
            long,
            default_value_t = p2p::DEFAULT_RELAY_MAX_RESERVATIONS,
            help = "Maximum active relay reservations"
        )]
        max_reservations: usize,

        #[arg(
            long,
            default_value_t = p2p::DEFAULT_RELAY_MAX_RESERVATIONS_PER_PEER,
            help = "Maximum active relay reservations per peer"
        )]
        max_reservations_per_peer: usize,

        #[arg(
            long,
            default_value_t = p2p::DEFAULT_RELAY_MAX_CIRCUITS,
            help = "Maximum active relay circuits"
        )]
        max_circuits: usize,

        #[arg(
            long,
            default_value_t = p2p::DEFAULT_RELAY_MAX_CIRCUITS_PER_PEER,
            help = "Maximum active relay circuits per peer"
        )]
        max_circuits_per_peer: usize,

        #[arg(
            long,
            default_value_t = p2p::DEFAULT_RELAY_MAX_CIRCUIT_DURATION_SEC,
            help = "Maximum seconds a relay circuit may stay open"
        )]
        max_circuit_duration_sec: u64,

        #[arg(
            long,
            default_value_t = p2p::DEFAULT_RELAY_MAX_CIRCUIT_BYTES,
            help = "Maximum bytes transferred by a relay circuit; 0 means unlimited"
        )]
        max_circuit_bytes: u64,

        #[arg(
            long = "rate-limits",
            default_value_t = false,
            help = "Keep libp2p default relay per-peer/per-IP rate limiters"
        )]
        rate_limits: bool,
    },
    #[command(about = "Write config and create local P2P key files")]
    Init {
        #[arg(long, help = "Overwrite an existing config.toml")]
        force: bool,

        #[arg(long, help = "Logical Rustory user id to write into config")]
        user_id: Option<String>,

        #[arg(long, help = "Device id to write into config")]
        device_id: Option<String>,

        #[arg(
            long,
            alias = "tracker",
            value_delimiter = ',',
            help = "Comma-separated tracker base URLs to write into config"
        )]
        trackers: Vec<String>,

        #[arg(long, help = "Relay multiaddr to write into config")]
        relay: Option<String>,

        #[arg(
            long,
            alias = "token",
            help = "Tracker bearer token to write into config"
        )]
        tracker_token: Option<String>,
    },
    #[command(about = "Diagnose local config, tools, keys, and connectivity")]
    Doctor {
        #[arg(
            long,
            default_value_t = false,
            help = "Print diagnostics as pretty JSON"
        )]
        json: bool,

        #[arg(
            long,
            default_value_t = false,
            help = "Safely fix local permissions and missing privacy defaults before reporting"
        )]
        auto_fix: bool,
    },
    #[command(about = "Import existing shell history into the local store")]
    Import {
        #[arg(
            long,
            default_value = "zsh",
            help = "History source format to import: bash, zsh, or hishtory"
        )]
        shell: String,

        #[arg(long, help = "History source path; defaults to the selected source")]
        path: Option<String>,

        #[arg(long, help = "Maximum newest parsed history entries to import")]
        limit: Option<usize>,

        #[arg(long, help = "Logical Rustory user id for imported entries")]
        user_id: Option<String>,

        #[arg(long, help = "Device id for imported entries")]
        device_id: Option<String>,

        #[arg(long, help = "Hostname for imported entries")]
        hostname: Option<String>,
    },
    #[command(about = "Remove old Hishtory files and startup hooks after migration")]
    CleanupHishtory {
        #[arg(
            long,
            default_value_t = false,
            help = "Actually remove files; default is a dry-run plan"
        )]
        apply: bool,

        #[arg(
            long,
            conflicts_with = "no_archive",
            help = "Directory where a backup copy is written before --apply deletion"
        )]
        archive_dir: Option<String>,

        #[arg(
            long,
            default_value_t = false,
            conflicts_with = "archive_dir",
            help = "Allow --apply without writing a backup copy"
        )]
        no_archive: bool,

        #[arg(
            long,
            hide = true,
            help = "Home directory override for tests and scripted cleanup"
        )]
        home: Option<String>,
    },
}

pub fn run() -> Result<()> {
    let app = App::parse();
    let mut config_load_error = None;
    let cfg = match config::load_default() {
        Ok(cfg) => cfg,
        Err(err) if can_continue_after_config_load_error(&app.cmd) => {
            config_load_error = Some(format!("{err:#}"));
            config::FileConfig::default()
        }
        Err(err) => return Err(err),
    };

    let db_path = normalize_opt_string(app.db_path)
        .or_else(|| env_nonempty("RUSTORY_DB_PATH"))
        .or_else(|| normalize_opt_string(cfg.db_path.clone()))
        .unwrap_or_else(|| storage::DEFAULT_DB_PATH.to_string());

    match app.cmd {
        Command::Serve {
            bind,
            token,
            allow_unauthenticated,
        } => {
            let token = resolve_http_sync_token(token);
            validate_http_sync_serve_auth(&bind, token.as_deref(), allow_unauthenticated)?;
            transport::serve(&bind, &db_path, transport::ServeConfig { token })?;
        }
        Command::Sync { peers, push, token } => {
            let device_id = resolve_device_id(&cfg);
            let token = resolve_http_sync_token(token);
            transport::sync(
                &peers,
                &db_path,
                push,
                Some(&device_id),
                transport::SyncConfig { token },
            )?;
        }
        Command::P2pServe {
            listen,
            identity_key,
            swarm_key,
            relay,
            trackers,
            tracker_token,
        } => {
            let psk = resolve_swarm_psk(swarm_key, &cfg)?;
            let identity = resolve_p2p_identity(identity_key, &cfg)?;
            let relay_addr = resolve_relay_addr(relay, &cfg)?;
            let trackers = resolve_trackers(trackers, &cfg)?;
            let tracker_token = resolve_tracker_token(tracker_token, &cfg)?;
            let meta = resolve_peer_meta(&cfg);

            p2p::serve(
                &listen,
                &db_path,
                p2p::ServeConfig {
                    identity,
                    psk,
                    relay_addr,
                    trackers,
                    tracker_token,
                    meta,
                },
            )?;
        }
        Command::P2pSync {
            peers,
            limit,
            push,
            watch,
            interval_sec,
            start_jitter_sec,
            max_peers_per_tick,
            req_attempts,
            req_timeout_base_sec,
            req_timeout_cap_sec,
            req_backoff_base_ms,
            identity_key,
            swarm_key,
            relay,
            trackers,
            tracker_token,
        } => {
            let identity = resolve_p2p_identity(identity_key, &cfg)?;
            let psk = resolve_swarm_psk(swarm_key, &cfg)?;
            let relay_addr = resolve_relay_addr(relay, &cfg)?;
            let trackers = resolve_trackers(trackers, &cfg)?;
            let tracker_token = resolve_tracker_token(tracker_token, &cfg)?;
            let user_id = resolve_user_id(&cfg);
            let device_id = resolve_device_id(&cfg);
            let request_retry_policy = resolve_p2p_request_retry_policy(
                req_attempts,
                req_timeout_base_sec,
                req_timeout_cap_sec,
                req_backoff_base_ms,
                &cfg,
            )?;

            let sync_cfg = p2p::SyncConfig {
                identity,
                psk,
                relay_addr,
                trackers,
                tracker_token,
                user_id: Some(user_id),
                device_id: Some(device_id),
                request_retry_policy,
                max_peers_per_tick,
            };

            if watch {
                let interval = Duration::from_secs(interval_sec.max(1));
                let start_jitter_sec = resolve_p2p_watch_start_jitter_sec(start_jitter_sec, &cfg)?;
                eprintln!(
                    "p2p-sync watch: interval={:?} start_jitter_sec={}",
                    interval, start_jitter_sec
                );
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                {
                    let stop = stop.clone();
                    ctrlc::set_handler(move || {
                        stop.store(true, std::sync::atomic::Ordering::SeqCst);
                    })
                    .context("set Ctrl-C/SIGTERM handler")?;
                }

                let sleep_with_stop = |duration: Duration, stop: &std::sync::atomic::AtomicBool| {
                    // 중지 신호에 빠르게 반응하기 위해 sleep을 1초 단위로 쪼갠다.
                    for _ in 0..duration.as_secs() {
                        if stop.load(std::sync::atomic::Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                };

                if start_jitter_sec > 0 {
                    let delay = rand::rng().random_range(0..=start_jitter_sec);
                    if delay > 0 {
                        eprintln!("p2p-sync watch: start jitter={delay}s");
                        sleep_with_stop(Duration::from_secs(delay), stop.as_ref());
                    }
                }

                while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                    if let Err(err) = p2p::sync(&peers, limit, &db_path, sync_cfg.clone(), push) {
                        eprintln!("warn: p2p-sync failed: {err:#}");
                    }

                    sleep_with_stop(interval, stop.as_ref());
                }

                eprintln!("p2p-sync watch: shutting down");
                return Ok(());
            } else {
                p2p::sync(&peers, limit, &db_path, sync_cfg, push)?;
            }
        }
        Command::Daemon {
            listen,
            identity_key,
            swarm_key,
            relay,
            trackers,
            tracker_token,
            limit,
            pull_only,
            interval_sec,
            start_jitter_sec,
            sync_start_delay_sec,
            max_peers_per_tick,
            preflight,
            req_attempts,
            req_timeout_base_sec,
            req_timeout_cap_sec,
            req_backoff_base_ms,
        } => {
            run_daemon(
                DaemonArgs {
                    listen,
                    identity_key,
                    swarm_key,
                    relay,
                    trackers,
                    tracker_token,
                    limit,
                    pull_only,
                    interval_sec,
                    start_jitter_sec,
                    sync_start_delay_sec,
                    max_peers_per_tick,
                    preflight,
                    req_attempts,
                    req_timeout_base_sec,
                    req_timeout_cap_sec,
                    req_backoff_base_ms,
                },
                &cfg,
                &db_path,
            )?;
        }
        Command::SwarmKey { swarm_key } => {
            let path = resolve_swarm_key_path(swarm_key, &cfg);
            let psk = config::load_or_generate_swarm_key(&path)?;
            let expanded = config::expand_home_path(&path)?;

            println!("swarm key path: {}", expanded.display());
            println!("swarm key fingerprint: {}", psk.fingerprint());
        }
        Command::Record {
            cmd,
            cwd,
            exit_code,
            duration_ms,
            shell,
            hostname,
            user_id,
            device_id,
            print_id,
        } => {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                return Ok(());
            }
            if let Some(pattern) = resolve_record_ignore_regex(&cfg) {
                match should_ignore_record_command(cmd, &pattern) {
                    Ok(true) => return Ok(()),
                    Ok(false) => {}
                    Err(err) => {
                        // 훅은 stderr를 버릴 수 있으므로, 실패 시에도 안전하게(= 기록 스킵) 동작한다.
                        eprintln!(
                            "warn: invalid record ignore regex: {err} (skipping record for safety)"
                        );
                        return Ok(());
                    }
                }
            }

            let store = storage::LocalStore::open(&db_path)?;
            let cwd = normalize_opt_string(cwd).unwrap_or_else(default_cwd);

            let hostname = normalize_opt_string(hostname)
                .or_else(|| env_nonempty("HOSTNAME"))
                .unwrap_or_else(|| "unknown".to_string());

            let shell = normalize_opt_string(shell)
                .or_else(default_shell)
                .unwrap_or_else(|| "unknown".to_string());

            let user_id = normalize_opt_string(user_id)
                .or_else(|| env_nonempty("RUSTORY_USER_ID"))
                .or_else(|| normalize_opt_string(cfg.user_id.clone()))
                .or_else(|| env_nonempty("USER"))
                .unwrap_or_else(|| "unknown".to_string());

            let device_id = normalize_opt_string(device_id)
                .or_else(|| env_nonempty("RUSTORY_DEVICE_ID"))
                .or_else(|| normalize_opt_string(cfg.device_id.clone()))
                .unwrap_or_else(|| hostname.clone());

            let entry = crate::core::Entry::new(crate::core::EntryInput {
                device_id,
                user_id,
                ts: time::OffsetDateTime::now_utc(),
                cmd: cmd.to_string(),
                cwd,
                exit_code,
                duration_ms,
                shell,
                hostname,
            });

            store.insert_entries(std::slice::from_ref(&entry))?;

            if print_id {
                println!("{}", entry.entry_id);
            }

            if let Err(err) = maybe_spawn_async_upload(&db_path, &cfg) {
                // 기록 성공을 우선하고, 비동기 업로드 트리거 실패는 경고로만 남긴다.
                eprintln!("warn: async upload trigger failed: {err:#}");
            }

            if let Err(err) = maybe_run_auto_prune(&store, &cfg) {
                // 기록 성공을 우선하고, 자동 보관 실패는 경고로만 남긴다.
                eprintln!("warn: auto prune failed: {err:#}");
            }
        }
        Command::Search { limit } => {
            let limit = resolve_search_limit(limit, &cfg)?;

            let store = storage::LocalStore::open(&db_path)?;
            let entries = store.list_recent(limit)?;
            match search::select_action(&entries, |entry_id| {
                store.delete_entries_by_ids(&[entry_id.to_string()], false)?;
                Ok(())
            })? {
                Some(search::SearchAction::Select(cmd)) => {
                    println!("{cmd}");
                }
                None => {}
            }
        }
        Command::Prune {
            older_than_days,
            keep_recent,
            dry_run,
        } => {
            let keep_recent = keep_recent.unwrap_or(0);
            let store = storage::LocalStore::open(&db_path)?;
            let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
            let cutoff_unix = compute_prune_cutoff_unix(now_unix, older_than_days)?;
            let stats = store.prune_entries_older_than(cutoff_unix, keep_recent, dry_run)?;

            if dry_run {
                println!(
                    "prune dry-run: older_than_days={} keep_recent={} cutoff_unix={} matched={} deleted={}",
                    older_than_days, keep_recent, cutoff_unix, stats.matched, stats.deleted
                );
            } else {
                println!(
                    "prune: older_than_days={} keep_recent={} cutoff_unix={} matched={} deleted={}",
                    older_than_days, keep_recent, cutoff_unix, stats.matched, stats.deleted
                );
            }
        }
        Command::Delete {
            entry_id,
            cmd_regex,
            dry_run,
            yes,
            vacuum,
        } => {
            let cmd_regex = normalize_opt_string(cmd_regex);
            let has_entry_id = entry_id.iter().any(|id| !id.trim().is_empty());
            if !has_entry_id && cmd_regex.is_none() {
                anyhow::bail!("delete requires --entry-id or --cmd-regex");
            }
            if !dry_run && !yes {
                anyhow::bail!(
                    "refusing to delete without --yes; run --dry-run first or pass --yes"
                );
            }

            let store = storage::LocalStore::open(&db_path)?;
            let mut selected_ids = entry_id;
            let mut selector_count = usize::from(has_entry_id);

            if let Some(pattern) = cmd_regex.as_deref() {
                let re = regex::Regex::new(pattern).context("invalid delete command regex")?;
                let mut regex_ids = store.entry_ids_matching_cmd_regex(&re)?;
                selected_ids.append(&mut regex_ids);
                selector_count += 1;
            }

            let stats = store.delete_entries_by_ids(&selected_ids, dry_run)?;
            let mut compacted = false;
            if vacuum && !dry_run {
                store.compact_storage()?;
                compacted = true;
            }

            if dry_run {
                println!(
                    "delete dry-run: selectors={} matched={} deleted=0",
                    selector_count, stats.matched
                );
            } else {
                println!(
                    "delete: selectors={} matched={} deleted={} compacted={}",
                    selector_count, stats.matched, stats.deleted, compacted
                );
            }
        }
        Command::SyncStatus {
            peer,
            json,
            with_tracker,
            watch,
            interval_sec,
        } => {
            if watch && json {
                anyhow::bail!("sync-status --watch does not support --json");
            }
            let peer = normalize_opt_string(peer);
            let store = storage::LocalStore::open(&db_path)?;
            let local_device_id = resolve_device_id(&cfg);
            let local_peer_id = resolve_local_p2p_peer_id(&cfg);
            let trackers = if with_tracker {
                Some(resolve_trackers(Vec::new(), &cfg)?)
            } else {
                None
            };
            let tracker_token = if with_tracker {
                resolve_tracker_token(None, &cfg)?
            } else {
                None
            };
            if watch {
                run_sync_status_watch(
                    &store,
                    &local_device_id,
                    local_peer_id.as_deref(),
                    peer.as_deref(),
                    trackers.as_deref(),
                    tracker_token.as_deref(),
                    interval_sec.max(1),
                )?;
                return Ok(());
            }

            let report = build_sync_status_report_for_cli(
                &store,
                &local_device_id,
                local_peer_id.as_deref(),
                peer.as_deref(),
                trackers.as_deref(),
                tracker_token.as_deref(),
            )?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).context("serialize sync-status json")?
                );
                return Ok(());
            }

            println!("local ingest head: {}", report.local_head);
            println!("local device id: {}", report.local_device_id);

            if report.peers.is_empty() {
                if let Some(peer_id) = peer.as_deref() {
                    println!("peer sync state: no state for peer '{peer_id}'");
                } else {
                    println!("peer sync state: (empty)");
                }
            } else {
                for status in report.peers {
                    let last_seen = status
                        .last_seen_unix
                        .map(|ts| ts.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    let last_seen_age = status
                        .last_seen_age_sec
                        .map(|age| age.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    println!(
                        "peer={} device={} pull_cursor={} push_cursor={} outbound_push_pending={} pending_push={} last_seen_unix={} last_seen_age_sec={}",
                        status.peer_id,
                        status.peer_device_id.as_deref().unwrap_or("-"),
                        status.pull_cursor,
                        status.push_cursor,
                        status.outbound_push_pending,
                        status.pending_push,
                        last_seen,
                        last_seen_age
                    );
                }
            }

            if let Some(trackers) = report.tracker_status {
                if trackers.is_empty() {
                    println!("tracker status: (none)");
                } else {
                    for tracker in trackers {
                        let latency = tracker
                            .latency_ms
                            .map(|ms| ms.to_string())
                            .unwrap_or_else(|| "-".to_string());
                        if let Some(error) = tracker.error {
                            println!(
                                "tracker={} reachable={} latency_ms={} error={error}",
                                tracker.base_url, tracker.reachable, latency
                            );
                        } else {
                            println!(
                                "tracker={} reachable={} latency_ms={}",
                                tracker.base_url, tracker.reachable, latency
                            );
                        }
                    }
                }
            }
        }
        Command::Version { json } => {
            print_build_info(json)?;
        }
        Command::Update {
            version,
            repo,
            asset_base_url,
            asset_url,
            checksum_url,
            sha256,
            install_path,
            dry_run,
            no_restart_daemon,
        } => {
            crate::self_update::run_update(crate::self_update::UpdateRequest {
                version,
                repo,
                asset_base_url,
                asset_url,
                checksum_url,
                sha256,
                install_path: install_path.map(std::path::PathBuf::from),
                dry_run,
                restart_daemon: !no_restart_daemon,
            })?;
        }
        Command::Hook { shell } => {
            let shell = hook::Shell::parse(shell.as_str())?;
            let content = hook::render_hook(shell);
            println!("{content}");
        }
        Command::TrackerServe {
            bind,
            ttl_sec,
            token,
            allow_unauthenticated,
        } => {
            let token = resolve_tracker_token(token, &cfg)?;
            validate_tracker_serve_auth(&bind, token.as_deref(), allow_unauthenticated)?;
            tracker::serve(&bind, ttl_sec, token)?;
        }
        Command::RelayServe {
            listen,
            identity_key,
            swarm_key,
            max_reservations,
            max_reservations_per_peer,
            max_circuits,
            max_circuits_per_peer,
            max_circuit_duration_sec,
            max_circuit_bytes,
            rate_limits,
        } => {
            let psk = resolve_swarm_psk(swarm_key, &cfg)?;
            let identity = resolve_relay_identity(identity_key, &cfg)?;
            p2p::relay_serve(
                &listen,
                p2p::RelayServeConfig {
                    identity,
                    psk,
                    limits: p2p::RelayLimits {
                        max_reservations,
                        max_reservations_per_peer,
                        max_circuits,
                        max_circuits_per_peer,
                        max_circuit_duration: std::time::Duration::from_secs(
                            max_circuit_duration_sec,
                        ),
                        max_circuit_bytes,
                        rate_limits,
                    },
                },
            )?;
        }
        Command::Init {
            force,
            user_id,
            device_id,
            trackers,
            relay,
            tracker_token,
        } => {
            if let Some(err) = config_load_error.as_deref() {
                eprintln!("warn: ignoring invalid config because --force was set: {err}");
            }
            run_init(
                InitArgs {
                    force,
                    user_id,
                    device_id,
                    trackers,
                    relay,
                    tracker_token,
                },
                &cfg,
                &db_path,
            )?;
        }
        Command::Doctor { json, auto_fix } => {
            run_doctor(&cfg, &db_path, json, auto_fix, config_load_error.as_deref())?;
        }
        Command::Import {
            shell,
            path,
            limit,
            user_id,
            device_id,
            hostname,
        } => {
            let shell = history_import::HistoryShell::parse(shell.as_str())?;
            let path = normalize_opt_string(path)
                .unwrap_or_else(|| shell.default_history_path().to_string());
            let path = config::expand_home_path(&path)?;

            let hostname = normalize_opt_string(hostname)
                .or_else(|| env_nonempty("HOSTNAME"))
                .unwrap_or_else(|| "unknown".to_string());

            let user_id = normalize_opt_string(user_id)
                .or_else(|| env_nonempty("RUSTORY_USER_ID"))
                .or_else(|| normalize_opt_string(cfg.user_id.clone()))
                .or_else(|| env_nonempty("USER"))
                .unwrap_or_else(|| "unknown".to_string());

            let device_id = normalize_opt_string(device_id)
                .or_else(|| env_nonempty("RUSTORY_DEVICE_ID"))
                .or_else(|| normalize_opt_string(cfg.device_id.clone()))
                .unwrap_or_else(|| hostname.clone());

            let ignore_re = match resolve_record_ignore_regex(&cfg) {
                Some(pattern) => match regex::Regex::new(&pattern) {
                    Ok(re) => Some(re),
                    Err(err) => {
                        eprintln!(
                            "warn: invalid record ignore regex: {err} (skipping import for safety)"
                        );
                        return Ok(());
                    }
                },
                None => None,
            };

            let store = storage::LocalStore::open(&db_path)?;
            let stats = if shell.is_hishtory() {
                history_import::import_hishtory_sqlite_into_store(
                    &store,
                    history_import::HishtoryImportRequest {
                        path: &path,
                        limit,
                        user_id: &user_id,
                        device_id: &device_id,
                        hostname: &hostname,
                        ignore_regex: ignore_re.as_ref(),
                    },
                )?
            } else {
                let content = history_import::read_history_file(&path)?;
                history_import::import_into_store(
                    &store,
                    history_import::ImportRequest {
                        shell,
                        content: &content,
                        limit,
                        user_id: &user_id,
                        device_id: &device_id,
                        hostname: &hostname,
                        ignore_regex: ignore_re.as_ref(),
                    },
                )?
            };

            println!(
                "import: path={} shell={} received={} inserted={} ignored={} skipped={}",
                path.display(),
                shell.as_str(),
                stats.received,
                stats.inserted,
                stats.ignored,
                stats.skipped
            );
        }
        Command::CleanupHishtory {
            apply,
            archive_dir,
            no_archive,
            home,
        } => {
            let home_dir = match normalize_opt_string(home) {
                Some(path) => config::expand_home_path(&path)?,
                None => std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .context("HOME env var not set")?,
            };
            let archive_dir = normalize_opt_string(archive_dir)
                .map(|path| config::expand_home_path(&path))
                .transpose()?;
            let report = hishtory_cleanup::cleanup_hishtory(hishtory_cleanup::CleanupOptions {
                home_dir,
                apply,
                archive_dir,
                no_archive,
                backup_name: None,
            })?;
            hishtory_cleanup::print_report(&report, io::stdout())?;
        }
    }

    Ok(())
}

fn can_continue_after_config_load_error(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::Doctor { .. }
            | Command::Version { .. }
            | Command::Update { .. }
            | Command::CleanupHishtory { .. }
            | Command::Init { force: true, .. }
    )
}

#[derive(Debug, Clone)]
struct DaemonArgs {
    listen: String,
    identity_key: Option<String>,
    swarm_key: Option<String>,
    relay: Option<String>,
    trackers: Vec<String>,
    tracker_token: Option<String>,
    limit: usize,
    pull_only: bool,
    interval_sec: u64,
    start_jitter_sec: Option<u64>,
    sync_start_delay_sec: u64,
    max_peers_per_tick: usize,
    preflight: bool,
    req_attempts: Option<u64>,
    req_timeout_base_sec: Option<u64>,
    req_timeout_cap_sec: Option<u64>,
    req_backoff_base_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DaemonChildSpecs {
    serve_args: Vec<String>,
    sync_args: Vec<String>,
    tracker_token_env: Option<String>,
}

#[derive(Debug, Clone)]
struct InitArgs {
    force: bool,
    user_id: Option<String>,
    device_id: Option<String>,
    trackers: Vec<String>,
    relay: Option<String>,
    tracker_token: Option<String>,
}

fn run_daemon(args: DaemonArgs, cfg: &config::FileConfig, db_path: &str) -> Result<()> {
    if args.limit == 0 {
        anyhow::bail!("daemon --limit must be >= 1");
    }

    let trackers = resolve_trackers(args.trackers.clone(), cfg)?;
    if trackers.is_empty() {
        anyhow::bail!(
            "daemon requires at least one tracker in --trackers, RUSTORY_TRACKERS, or config.toml"
        );
    }

    if resolve_relay_addr(args.relay.clone(), cfg)?.is_none() {
        anyhow::bail!("daemon requires relay_addr for tracker-based sync");
    }

    // Fail before spawning children when the durable daily-driver inputs are broken.
    let _ = resolve_swarm_psk(args.swarm_key.clone(), cfg)?;
    let _ = resolve_p2p_identity(args.identity_key.clone(), cfg)?;
    let tracker_token = resolve_tracker_token(args.tracker_token.clone(), cfg)?;
    let _ = resolve_p2p_watch_start_jitter_sec(args.start_jitter_sec, cfg)?;
    let _ = resolve_p2p_request_retry_policy(
        args.req_attempts,
        args.req_timeout_base_sec,
        args.req_timeout_cap_sec,
        args.req_backoff_base_ms,
        cfg,
    )?;

    if args.preflight {
        run_daemon_preflight(&trackers, tracker_token.as_deref())?;
    }

    let specs = build_daemon_child_specs(db_path, &args);
    let exe = std::env::current_exe().context("resolve current executable for daemon")?;
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc::set_handler(move || {
            stop.store(true, Ordering::SeqCst);
        })
        .context("set Ctrl-C/SIGTERM handler")?;
    }

    eprintln!(
        "daemon: starting p2p-serve and p2p-sync watch (push={})",
        !args.pull_only
    );
    let mut serve = spawn_daemon_child(
        "p2p-serve",
        &exe,
        &specs.serve_args,
        specs.tracker_token_env.as_deref(),
    )?;

    sleep_with_stop(
        Duration::from_secs(args.sync_start_delay_sec),
        stop.as_ref(),
    );
    if stop.load(Ordering::SeqCst) {
        terminate_daemon_child("p2p-serve", &mut serve)?;
        return Ok(());
    }
    if let Some(status) = serve.try_wait().context("poll p2p-serve child")? {
        anyhow::bail!("daemon child p2p-serve exited before sync start: {status}");
    }

    let mut sync = spawn_daemon_child(
        "p2p-sync",
        &exe,
        &specs.sync_args,
        specs.tracker_token_env.as_deref(),
    )?;

    supervise_daemon_children(&mut serve, &mut sync, stop.as_ref())
}

fn run_daemon_preflight(trackers: &[String], tracker_token: Option<&str>) -> Result<()> {
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

fn validate_daemon_preflight_statuses(statuses: &[SyncStatusTrackerReport]) -> Result<()> {
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

fn build_daemon_child_specs(db_path: &str, args: &DaemonArgs) -> DaemonChildSpecs {
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

fn spawn_daemon_child(
    label: &str,
    exe: &std::path::Path,
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

fn redacted_command(exe: &std::path::Path, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(exe.display().to_string());
    parts.extend(args.iter().cloned());
    parts.join(" ")
}

fn supervise_daemon_children(serve: &mut Child, sync: &mut Child, stop: &AtomicBool) -> Result<()> {
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

fn sleep_with_stop(duration: Duration, stop: &AtomicBool) {
    let deadline = Instant::now() + duration;
    while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn terminate_daemon_child(label: &str, child: &mut Child) -> Result<()> {
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

fn run_init(args: InitArgs, cfg: &config::FileConfig, db_path: &str) -> Result<()> {
    let cfg_path = config::expand_home_path(config::DEFAULT_CONFIG_PATH)?;
    let cfg_exists = std::fs::metadata(&cfg_path).is_ok();

    if cfg_exists && !args.force {
        println!(
            "config already exists: {} (use --force to overwrite)",
            cfg_path.display()
        );
    } else {
        let rendered = render_config_toml(&args, cfg, db_path)?;
        if let Some(parent) = cfg_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir: {}", parent.display()))?;
        }
        std::fs::write(&cfg_path, rendered)
            .with_context(|| format!("write config: {}", cfg_path.display()))?;
        restrict_permissions_0600(&cfg_path)?;
        println!("wrote config: {}", cfg_path.display());
    }

    // 키는 config/env/CLI 우선순위를 그대로 따른다. (p2p 커맨드와 동일한 규칙)
    let swarm_key_path = resolve_swarm_key_path(None, cfg);
    let swarm_key_abs = config::expand_home_path(&swarm_key_path)?;
    let swarm_existed = std::fs::metadata(&swarm_key_abs).is_ok();
    let psk = config::load_or_generate_swarm_key(&swarm_key_path)?;
    println!("swarm key path: {}", swarm_key_abs.display());
    println!("swarm key fingerprint: {}", psk.fingerprint());
    if !swarm_existed {
        println!(
            "note: 기존 swarm에 붙이는 신규 디바이스라면, 다른 디바이스의 swarm.key를 이 경로로 복사해야 한다."
        );
    }

    let p2p_identity_key_path = resolve_p2p_identity_key_path(None, cfg);
    let p2p_identity_abs = config::expand_home_path(&p2p_identity_key_path)?;
    let id_existed = std::fs::metadata(&p2p_identity_abs).is_ok();
    let identity = config::load_or_generate_identity_keypair(&p2p_identity_key_path)?;
    let peer_id = identity.public().to_peer_id();
    println!("p2p identity key path: {}", p2p_identity_abs.display());
    println!("p2p peer id: {peer_id}");
    if !id_existed {
        println!("note: p2p identity key는 디바이스별로 고유해야 한다(공유하지 않음).");
    }

    println!("next:");
    println!("- 설정 확인: rr doctor");
    println!("- 상시 동기화: rr daemon");
    println!("- 분리 운영: rr p2p-serve + rr p2p-sync --watch --push");

    Ok(())
}

fn render_config_toml(args: &InitArgs, cfg: &config::FileConfig, db_path: &str) -> Result<String> {
    // 값 결정(가능하면 기존 config/입력값을 반영).
    let user_id = normalize_opt_string(args.user_id.clone())
        .or_else(|| normalize_opt_string(cfg.user_id.clone()))
        .or_else(|| env_nonempty("USER"));

    let device_id = normalize_opt_string(args.device_id.clone())
        .or_else(|| normalize_opt_string(cfg.device_id.clone()))
        .or_else(|| env_nonempty("HOSTNAME"))
        .or_else(|| env_nonempty("HOST"));

    let trackers = resolve_trackers(args.trackers.clone(), cfg)?;

    let relay_addr = normalize_opt_string(args.relay.clone())
        .or_else(|| normalize_opt_string(cfg.relay_addr.clone()))
        .or_else(|| env_nonempty("RUSTORY_RELAY_ADDR"));

    if let Some(relay) = relay_addr.as_deref() {
        // 잘못된 값을 config에 쓰지 않도록 미리 파싱 검증한다.
        let _: libp2p::Multiaddr = relay.parse().context("parse relay multiaddr")?;
    }

    let tracker_token = normalize_opt_string(args.tracker_token.clone())
        .or_else(|| normalize_opt_string(cfg.tracker_token.clone()))
        .or_else(|| env_nonempty("RUSTORY_TRACKER_TOKEN"));
    if let Some(token) = tracker_token.as_deref() {
        tracker::validate_tracker_token_value(token, "tracker token")?;
    }

    let swarm_key_path = normalize_opt_string(cfg.swarm_key_path.clone())
        .unwrap_or_else(|| config::DEFAULT_SWARM_KEY_PATH.to_string());
    let p2p_identity_key_path = normalize_opt_string(cfg.p2p_identity_key_path.clone())
        .unwrap_or_else(|| config::DEFAULT_P2P_IDENTITY_KEY_PATH.to_string());
    let relay_identity_key_path = normalize_opt_string(cfg.relay_identity_key_path.clone())
        .unwrap_or_else(|| config::DEFAULT_RELAY_IDENTITY_KEY_PATH.to_string());

    let mut out = String::new();
    out.push_str("# rustory config.toml\n");
    out.push_str("# generated by `rr init`\n\n");

    out.push_str(&format!("db_path = {db_path:?}\n"));

    if let Some(v) = user_id.as_deref() {
        out.push_str(&format!("user_id = {v:?}\n"));
    } else {
        out.push_str("# user_id = \"your-user\"\n");
    }

    if let Some(v) = device_id.as_deref() {
        out.push_str(&format!("device_id = {v:?}\n"));
    } else {
        out.push_str("# device_id = \"your-device\"\n");
    }
    out.push('\n');

    if !trackers.is_empty() {
        out.push_str("trackers = [\n");
        for t in trackers {
            out.push_str(&format!("  {t:?},\n"));
        }
        out.push_str("]\n");
    } else {
        out.push_str("# trackers = [\"http://127.0.0.1:8850\"]\n");
    }

    if let Some(v) = relay_addr.as_deref() {
        out.push_str(&format!("relay_addr = {v:?}\n"));
    } else {
        out.push_str("# relay_addr = \"/ip4/127.0.0.1/tcp/4001/p2p/<relay_peer_id>\"\n");
    }

    if let Some(v) = tracker_token.as_deref() {
        out.push_str(&format!("tracker_token = {v:?}\n"));
    } else {
        out.push_str("# tracker_token = \"secret\" # optional\n");
    }
    out.push('\n');

    out.push_str(&format!("swarm_key_path = {swarm_key_path:?}\n"));
    out.push_str(&format!(
        "p2p_identity_key_path = {p2p_identity_key_path:?}\n"
    ));
    out.push_str(&format!(
        "relay_identity_key_path = {relay_identity_key_path:?}\n"
    ));
    out.push('\n');

    out.push_str("# p2p_watch_start_jitter_sec = 10 # optional\n");
    out.push_str("# p2p_request_attempts = 3 # optional\n");
    out.push_str("# p2p_request_timeout_base_sec = 5 # optional\n");
    out.push_str("# p2p_request_timeout_cap_sec = 30 # optional\n");
    out.push_str("# p2p_request_backoff_base_ms = 200 # optional\n");
    out.push_str("# search_limit_default = 100000 # optional\n");
    let record_ignore_regex = normalize_opt_string(cfg.record_ignore_regex.clone())
        .unwrap_or_else(|| DEFAULT_RECORD_IGNORE_REGEX.to_string());
    out.push_str(&format!("record_ignore_regex = {record_ignore_regex:?}\n"));
    out.push_str("# async_upload = false # optional; env RUSTORY_ASYNC_UPLOAD overrides\n");
    out.push_str("# async_upload_interval_sec = 15 # optional\n");
    out.push_str("# async_upload_limit = 200 # optional\n");
    out.push_str(
        "# async_upload_marker_path = \"~/.config/rustory/async-upload.last\" # optional\n",
    );
    out.push_str("# auto_prune = false # optional; env RUSTORY_AUTO_PRUNE overrides\n");
    out.push_str("# auto_prune_days = 180 # optional\n");
    out.push_str("# auto_prune_interval_sec = 86400 # optional\n");
    out.push_str("# auto_prune_keep_recent = 0 # optional\n");
    out.push_str("# auto_prune_marker_path = \"~/.config/rustory/auto-prune.last\" # optional\n");

    Ok(out)
}

fn restrict_permissions_0600(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("chmod 0600: {}", path.display()))?;
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AsyncUploadRuntimeSettings {
    enabled: bool,
    interval_sec: u64,
    limit: usize,
    marker_path: std::path::PathBuf,
    last_trigger_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutoPruneRuntimeSettings {
    enabled: bool,
    older_than_days: u64,
    interval_sec: u64,
    keep_recent: usize,
    marker_path: std::path::PathBuf,
    last_trigger_unix: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct AsyncUploadDoctorReport {
    enabled: bool,
    interval_sec: u64,
    limit: usize,
    marker_path: std::path::PathBuf,
    last_trigger_unix: Option<i64>,
    next_due_in_sec: u64,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct AutoPruneDoctorReport {
    enabled: bool,
    older_than_days: u64,
    interval_sec: u64,
    keep_recent: usize,
    marker_path: std::path::PathBuf,
    last_trigger_unix: Option<i64>,
    next_due_in_sec: u64,
}

fn load_async_upload_runtime_settings(
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

fn summarize_async_upload_runtime(
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

fn load_auto_prune_runtime_settings(cfg: &config::FileConfig) -> Result<AutoPruneRuntimeSettings> {
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

fn summarize_auto_prune_runtime(
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

fn compute_next_due_in_sec(
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

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorReport {
    build: BuildInfoReport,
    config_path: String,
    config_exists: bool,
    config_mode: Option<u32>,
    config_warning: Option<String>,
    config_error: Option<String>,
    db_path: String,
    db: DoctorDbStatusReport,
    user_id: String,
    device_id: String,
    hook: DoctorHookStatusReport,
    p2p_request_retry: DoctorP2pRequestRetryReport,
    record_ignore_regex: DoctorRecordIgnoreRegexReport,
    async_upload: DoctorAsyncUploadStatusReport,
    auto_prune: DoctorAutoPruneStatusReport,
    swarm_key: DoctorKeyStatusReport,
    p2p_identity_key: DoctorKeyStatusReport,
    relay_identity_key: DoctorKeyStatusReport,
    relay_addr: DoctorRelayAddrReport,
    tracker_token: DoctorTrackerTokenReport,
    trackers: Vec<SyncStatusTrackerReport>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorDbStatusReport {
    path: String,
    exists: bool,
    entry_count: Option<usize>,
    latest_ingest_seq: Option<i64>,
    peer_book_count: Option<usize>,
    sync_peer_count: Option<usize>,
    db_mode: Option<u32>,
    parent_mode: Option<u32>,
    permission_warning: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorP2pRequestRetryReport {
    attempts: Option<usize>,
    timeout_base_sec: Option<u64>,
    timeout_cap_sec: Option<u64>,
    backoff_base_ms: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorHookStatusReport {
    shell: Option<String>,
    installed: bool,
    disabled: bool,
    search_limit: Option<usize>,
    warnings: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorRecordIgnoreRegexReport {
    pattern: Option<String>,
    valid: bool,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorAsyncUploadStatusReport {
    enabled: Option<bool>,
    interval_sec: Option<u64>,
    limit: Option<usize>,
    marker_path: Option<String>,
    last_trigger_unix: Option<i64>,
    next_due_in_sec: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorAutoPruneStatusReport {
    enabled: Option<bool>,
    older_than_days: Option<u64>,
    interval_sec: Option<u64>,
    keep_recent: Option<usize>,
    marker_path: Option<String>,
    last_trigger_unix: Option<i64>,
    next_due_in_sec: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorKeyStatusReport {
    path: String,
    exists: bool,
    value: Option<String>,
    load_error: Option<String>,
    mode: Option<u32>,
    warning: Option<String>,
    stat_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorRelayAddrReport {
    value: Option<String>,
    warning: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct DoctorTrackerTokenReport {
    configured: bool,
    length: Option<usize>,
    warning: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct BuildInfoReport {
    version: String,
    build_revision: String,
    build_revision_source: String,
    build_dirty: bool,
}

fn build_info_report() -> BuildInfoReport {
    BuildInfoReport {
        version: crate::build_info::VERSION.to_string(),
        build_revision: crate::build_info::BUILD_REVISION.to_string(),
        build_revision_source: crate::build_info::BUILD_REVISION_SOURCE.to_string(),
        build_dirty: crate::build_info::build_dirty(),
    }
}

fn print_build_info(json: bool) -> Result<()> {
    let report = build_info_report();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).context("serialize version json")?
        );
        return Ok(());
    }

    println!("version: {}", report.version);
    println!("build_revision: {}", report.build_revision);
    println!("build_revision_source: {}", report.build_revision_source);
    println!("build_dirty: {}", report.build_dirty);

    Ok(())
}

fn build_doctor_report(
    cfg: &config::FileConfig,
    db_path: &str,
    config_error: Option<&str>,
) -> Result<DoctorReport> {
    let cfg_path = config::expand_home_path(config::DEFAULT_CONFIG_PATH)?;
    let cfg_exists = match std::fs::metadata(&cfg_path) {
        Ok(_) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => false,
    };
    let config_mode = file_mode_777(&cfg_path);
    let config_warning = build_config_file_warning(cfg_exists, config_mode);

    let db = build_db_status_report(db_path)?;
    let user_id = resolve_user_id(cfg);
    let device_id = resolve_device_id(cfg);
    let hook = build_hook_status_report(cfg);
    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();

    let p2p_request_retry = match resolve_p2p_request_retry_policy(None, None, None, None, cfg) {
        Ok(policy) => DoctorP2pRequestRetryReport {
            attempts: Some(policy.attempts),
            timeout_base_sec: Some(policy.timeout_base.as_secs()),
            timeout_cap_sec: Some(policy.timeout_cap.as_secs()),
            backoff_base_ms: Some(
                u64::try_from(policy.backoff_base.as_millis()).unwrap_or(u64::MAX),
            ),
            error: None,
        },
        Err(err) => DoctorP2pRequestRetryReport {
            attempts: None,
            timeout_base_sec: None,
            timeout_cap_sec: None,
            backoff_base_ms: None,
            error: Some(format!("{err:#}")),
        },
    };

    let record_ignore_regex = match resolve_record_ignore_regex(cfg) {
        Some(pattern) => match regex::Regex::new(&pattern) {
            Ok(_) => DoctorRecordIgnoreRegexReport {
                pattern: Some(pattern),
                valid: true,
                error: None,
            },
            Err(err) => DoctorRecordIgnoreRegexReport {
                pattern: Some(pattern),
                valid: false,
                error: Some(err.to_string()),
            },
        },
        None => DoctorRecordIgnoreRegexReport {
            pattern: None,
            valid: true,
            error: None,
        },
    };

    let async_upload = match load_async_upload_runtime_settings(cfg) {
        Ok(settings) => {
            let report = summarize_async_upload_runtime(settings, now_unix);
            DoctorAsyncUploadStatusReport {
                enabled: Some(report.enabled),
                interval_sec: Some(report.interval_sec),
                limit: Some(report.limit),
                marker_path: Some(report.marker_path.display().to_string()),
                last_trigger_unix: report.last_trigger_unix,
                next_due_in_sec: Some(report.next_due_in_sec),
                error: None,
            }
        }
        Err(err) => DoctorAsyncUploadStatusReport {
            enabled: None,
            interval_sec: None,
            limit: None,
            marker_path: None,
            last_trigger_unix: None,
            next_due_in_sec: None,
            error: Some(format!("{err:#}")),
        },
    };

    let auto_prune = match load_auto_prune_runtime_settings(cfg) {
        Ok(settings) => {
            let report = summarize_auto_prune_runtime(settings, now_unix);
            DoctorAutoPruneStatusReport {
                enabled: Some(report.enabled),
                older_than_days: Some(report.older_than_days),
                interval_sec: Some(report.interval_sec),
                keep_recent: Some(report.keep_recent),
                marker_path: Some(report.marker_path.display().to_string()),
                last_trigger_unix: report.last_trigger_unix,
                next_due_in_sec: Some(report.next_due_in_sec),
                error: None,
            }
        }
        Err(err) => DoctorAutoPruneStatusReport {
            enabled: None,
            older_than_days: None,
            interval_sec: None,
            keep_recent: None,
            marker_path: None,
            last_trigger_unix: None,
            next_due_in_sec: None,
            error: Some(format!("{err:#}")),
        },
    };

    let swarm_key_path = resolve_swarm_key_path(None, cfg);
    let (swarm_value, swarm_load_error) = match config::load_swarm_key(&swarm_key_path) {
        Ok(value) => (
            value.map(|key| key.fingerprint().to_string()),
            None::<String>,
        ),
        Err(err) => (None, Some(format!("{err:#}"))),
    };
    let swarm_key = build_key_status_report(&swarm_key_path, swarm_value, swarm_load_error)?;

    let p2p_identity_key_path = resolve_p2p_identity_key_path(None, cfg);
    let (p2p_identity_value, p2p_identity_load_error) =
        match config::load_identity_keypair(&p2p_identity_key_path) {
            Ok(value) => (
                value.map(|key| key.public().to_peer_id().to_string()),
                None::<String>,
            ),
            Err(err) => (None, Some(format!("{err:#}"))),
        };
    let p2p_identity_key = build_key_status_report(
        &p2p_identity_key_path,
        p2p_identity_value,
        p2p_identity_load_error,
    )?;

    let relay_identity_key_path = resolve_relay_identity_key_path(None, cfg);
    let (relay_identity_value, relay_identity_load_error) =
        match config::load_identity_keypair(&relay_identity_key_path) {
            Ok(value) => (
                value.map(|key| key.public().to_peer_id().to_string()),
                None::<String>,
            ),
            Err(err) => (None, Some(format!("{err:#}"))),
        };
    let relay_identity_key = build_key_status_report(
        &relay_identity_key_path,
        relay_identity_value,
        relay_identity_load_error,
    )?;

    let relay_addr = match resolve_relay_addr(None, cfg) {
        Ok(Some(addr)) => DoctorRelayAddrReport {
            warning: relay_addr_reachability_warning(&addr),
            value: Some(addr.to_string()),
            error: None,
        },
        Ok(None) => DoctorRelayAddrReport {
            value: None,
            warning: None,
            error: None,
        },
        Err(err) => DoctorRelayAddrReport {
            value: None,
            warning: None,
            error: Some(format!("{err:#}")),
        },
    };

    let trackers = resolve_trackers(Vec::new(), cfg)?;
    let tracker_token_raw = resolve_tracker_token_raw(None, cfg);
    let tracker_token = validate_resolved_tracker_token(tracker_token_raw.clone())
        .ok()
        .flatten();
    let tracker_token_report =
        build_tracker_token_report(tracker_token_raw.as_deref(), !trackers.is_empty());
    let trackers = build_tracker_status_report(&trackers, tracker_token.as_deref());

    Ok(DoctorReport {
        build: build_info_report(),
        config_path: cfg_path.display().to_string(),
        config_exists: cfg_exists,
        config_mode,
        config_warning,
        config_error: config_error.map(|err| err.to_string()),
        db_path: db.path.clone(),
        db,
        user_id,
        device_id,
        hook,
        p2p_request_retry,
        record_ignore_regex,
        async_upload,
        auto_prune,
        swarm_key,
        p2p_identity_key,
        relay_identity_key,
        relay_addr,
        tracker_token: tracker_token_report,
        trackers,
    })
}

fn build_db_status_report(db_path: &str) -> Result<DoctorDbStatusReport> {
    let inspection = storage::inspect_existing_store(db_path)?;
    let permissions = storage::inspect_store_permissions(db_path)?;
    Ok(DoctorDbStatusReport {
        path: inspection.path.display().to_string(),
        exists: inspection.exists,
        entry_count: inspection.entry_count,
        latest_ingest_seq: inspection.latest_ingest_seq,
        peer_book_count: inspection.peer_book_count,
        sync_peer_count: inspection.sync_peer_count,
        db_mode: permissions.db_mode,
        parent_mode: permissions.parent_mode,
        permission_warning: permissions.warning,
        error: inspection.error,
    })
}

fn build_config_file_warning(exists: bool, mode: Option<u32>) -> Option<String> {
    if !exists {
        return None;
    }

    let mode = mode?;
    if mode != 0o600 {
        return Some(format!(
            "mode={mode:03o}, want 600 because config.toml can contain tracker_token"
        ));
    }

    None
}

fn build_tracker_token_report(
    token: Option<&str>,
    trackers_configured: bool,
) -> DoctorTrackerTokenReport {
    let Some(token) = token else {
        return DoctorTrackerTokenReport {
            configured: false,
            length: None,
            warning: if trackers_configured {
                Some(
                    "missing; tracker requests will be unauthenticated unless the tracker also has no token"
                        .to_string(),
                )
            } else {
                None
            },
            error: None,
        };
    };

    let error = tracker::validate_tracker_token_value(token, "tracker token")
        .err()
        .map(|err| format!("{err:#}"));
    let mut warnings = Vec::new();
    if token.len() < 32 {
        warnings.push("short; use at least 32 random characters for production".to_string());
    }
    if tracker::has_literal_quote_wrapper(token) {
        warnings.push(
            "appears wrapped in literal quote characters; pass the raw token value".to_string(),
        );
    }
    if token.to_ascii_lowercase().starts_with("bearer ") {
        warnings.push("contains Bearer prefix; configure only the raw token".to_string());
    }
    if token.chars().any(char::is_whitespace) {
        warnings.push(
            "contains whitespace; quote exactly and prefer tokens without whitespace".to_string(),
        );
    }
    if !token.is_ascii() {
        warnings.push(
            "contains non-ASCII characters; HTTP clients or proxies may reject it".to_string(),
        );
    }

    DoctorTrackerTokenReport {
        configured: true,
        length: Some(token.len()),
        warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
        error,
    }
}

fn build_key_status_report(
    path: &str,
    value: Option<String>,
    load_error: Option<String>,
) -> Result<DoctorKeyStatusReport> {
    let expanded = config::expand_home_path(path)?;
    let path_str = expanded.display().to_string();

    let (exists, stat_error) = match std::fs::metadata(&expanded) {
        Ok(_) => (true, None),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (false, None),
        Err(err) => (false, Some(err.to_string())),
    };

    let mode = if exists {
        file_mode_777(&expanded)
    } else {
        None
    };
    let warning = mode.and_then(|resolved| {
        if resolved != 0o600 {
            Some(format!("mode={resolved:03o}, want 600"))
        } else {
            None
        }
    });

    Ok(DoctorKeyStatusReport {
        path: path_str,
        exists,
        value,
        load_error,
        mode,
        warning,
        stat_error,
    })
}

#[derive(Debug, Default)]
struct DoctorAutoFixReport {
    actions: Vec<DoctorAutoFixAction>,
}

#[derive(Debug)]
struct DoctorAutoFixAction {
    target: String,
    status: DoctorAutoFixStatus,
    detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorAutoFixStatus {
    Fixed,
    Ok,
    Skipped,
}

fn run_doctor(
    cfg: &config::FileConfig,
    db_path: &str,
    json: bool,
    auto_fix: bool,
    config_error: Option<&str>,
) -> Result<()> {
    if json && auto_fix {
        anyhow::bail!("doctor --auto-fix does not support --json");
    }

    let fixed_cfg;
    let cfg = if auto_fix {
        let report = run_doctor_auto_fix(cfg, db_path, config_error)?;
        print_doctor_auto_fix_report(&report);
        fixed_cfg = match config::load_default() {
            Ok(cfg) => cfg,
            Err(_) => cfg.clone(),
        };
        &fixed_cfg
    } else {
        cfg
    };

    if json {
        let report = build_doctor_report(cfg, db_path, config_error)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&report).context("serialize doctor json")?
        );
        return Ok(());
    }

    let cfg_path = config::expand_home_path(config::DEFAULT_CONFIG_PATH)?;
    let cfg_exists = match std::fs::metadata(&cfg_path) {
        Ok(_) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            eprintln!(
                "warn: cannot stat config path {}: {err}",
                cfg_path.display()
            );
            false
        }
    };

    let user_id = resolve_user_id(cfg);
    let device_id = resolve_device_id(cfg);
    let db = build_db_status_report(db_path)?;
    let build = build_info_report();

    println!(
        "build status: version={} revision={} source={} dirty={}",
        build.version, build.build_revision, build.build_revision_source, build.build_dirty
    );

    println!("config path: {} (exists: {cfg_exists})", cfg_path.display());
    match config_error {
        Some(err) => println!("config status: invalid: {err}"),
        None if cfg_exists => println!("config status: ok"),
        None => println!("config status: missing (using defaults/env)"),
    }
    if cfg_exists {
        let config_mode = file_mode_777(&cfg_path);
        let mode = config_mode
            .map(|value| format!("{value:03o}"))
            .unwrap_or_else(|| "-".to_string());
        match build_config_file_warning(cfg_exists, config_mode) {
            Some(warning) => println!("config permissions: mode={mode} warn: {warning}"),
            None => println!("config permissions: mode={mode}"),
        }
    }
    println!("db path: {}", db.path);
    print_db_status(&db);
    println!("user_id: {user_id}");
    println!("device_id: {device_id}");
    print_hook_status(&build_hook_status_report(cfg));
    match resolve_p2p_request_retry_policy(None, None, None, None, cfg) {
        Ok(request_retry_policy) => {
            println!(
                "p2p request retry: attempts={} timeout_base={:?} timeout_cap={:?} backoff_base={:?}",
                request_retry_policy.attempts,
                request_retry_policy.timeout_base,
                request_retry_policy.timeout_cap,
                request_retry_policy.backoff_base
            );
        }
        Err(err) => println!("p2p request retry: invalid: {err:#}"),
    }
    match resolve_record_ignore_regex(cfg) {
        Some(pattern) => match regex::Regex::new(&pattern) {
            Ok(_) => println!("record ignore regex: {pattern}"),
            Err(err) => {
                println!("record ignore regex: invalid: {err} (skipping record for safety)")
            }
        },
        None => println!("record ignore regex: (none)"),
    }
    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    match load_async_upload_runtime_settings(cfg) {
        Ok(settings) => {
            let report = summarize_async_upload_runtime(settings, now_unix);
            let last_trigger = report
                .last_trigger_unix
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string());
            println!(
                "async upload: enabled={} interval_sec={} limit={} marker_path={} last_trigger_unix={} next_due_in_sec={}",
                report.enabled,
                report.interval_sec,
                report.limit,
                report.marker_path.display(),
                last_trigger,
                report.next_due_in_sec,
            );
        }
        Err(err) => println!("async upload: invalid: {err:#}"),
    }
    match load_auto_prune_runtime_settings(cfg) {
        Ok(settings) => {
            let report = summarize_auto_prune_runtime(settings, now_unix);
            let last_trigger = report
                .last_trigger_unix
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string());
            println!(
                "auto prune: enabled={} older_than_days={} interval_sec={} keep_recent={} marker_path={} last_trigger_unix={} next_due_in_sec={}",
                report.enabled,
                report.older_than_days,
                report.interval_sec,
                report.keep_recent,
                report.marker_path.display(),
                last_trigger,
                report.next_due_in_sec,
            );
        }
        Err(err) => println!("auto prune: invalid: {err:#}"),
    }

    let swarm_key_path = resolve_swarm_key_path(None, cfg);
    let (swarm_fp, swarm_load_error) = match config::load_swarm_key(&swarm_key_path) {
        Ok(value) => (
            value.map(|key| key.fingerprint().to_string()),
            None::<String>,
        ),
        Err(err) => (None, Some(format!("{err:#}"))),
    };
    print_key_status(
        "swarm key",
        &swarm_key_path,
        swarm_fp.as_deref(),
        swarm_load_error.as_deref(),
    )?;

    let p2p_identity_key_path = resolve_p2p_identity_key_path(None, cfg);
    let (p2p_peer_id, p2p_load_error) = match config::load_identity_keypair(&p2p_identity_key_path)
    {
        Ok(value) => (
            value.map(|key| key.public().to_peer_id().to_string()),
            None::<String>,
        ),
        Err(err) => (None, Some(format!("{err:#}"))),
    };
    print_key_status(
        "p2p identity key",
        &p2p_identity_key_path,
        p2p_peer_id.as_deref(),
        p2p_load_error.as_deref(),
    )?;

    let relay_identity_key_path = resolve_relay_identity_key_path(None, cfg);
    let (relay_peer_id, relay_load_error) =
        match config::load_identity_keypair(&relay_identity_key_path) {
            Ok(value) => (
                value.map(|key| key.public().to_peer_id().to_string()),
                None::<String>,
            ),
            Err(err) => (None, Some(format!("{err:#}"))),
        };
    print_key_status(
        "relay identity key",
        &relay_identity_key_path,
        relay_peer_id.as_deref(),
        relay_load_error.as_deref(),
    )?;

    match resolve_relay_addr(None, cfg) {
        Ok(Some(addr)) => match relay_addr_reachability_warning(&addr) {
            Some(warning) => println!("relay addr: {addr} warn: {warning}"),
            None => println!("relay addr: {addr}"),
        },
        Ok(None) => println!("relay addr: (none)"),
        Err(err) => println!("relay addr: invalid: {err:#}"),
    }

    let trackers = resolve_trackers(Vec::new(), cfg)?;
    let token_raw = resolve_tracker_token_raw(None, cfg);
    print_tracker_token_status(&build_tracker_token_report(
        token_raw.as_deref(),
        !trackers.is_empty(),
    ));
    if trackers.is_empty() {
        println!("trackers: (none)");
        return Ok(());
    }

    let token = match validate_resolved_tracker_token(token_raw) {
        Ok(token) => token,
        Err(err) => {
            for base_url in trackers {
                println!("- {base_url} (ping: skipped: invalid tracker token: {err:#})");
            }
            return Ok(());
        }
    };
    println!("trackers:");
    for base_url in trackers {
        let ping = tracker_ping(&base_url, token.as_deref());
        match ping {
            Ok(latency_ms) => println!("- {base_url} (ping: ok, latency_ms={latency_ms})"),
            Err(err) => println!("- {base_url} (ping: fail: {err})"),
        }
    }

    Ok(())
}

fn run_doctor_auto_fix(
    cfg: &config::FileConfig,
    db_path: &str,
    config_error: Option<&str>,
) -> Result<DoctorAutoFixReport> {
    let mut report = DoctorAutoFixReport::default();
    let cfg_path = config::expand_home_path(config::DEFAULT_CONFIG_PATH)?;

    if let Some(parent) = cfg_path.parent() {
        fix_path_mode(parent, 0o700, "config parent", &mut report)?;
    }
    fix_path_mode(&cfg_path, 0o600, "config file", &mut report)?;
    fix_default_record_ignore_regex(cfg, &cfg_path, config_error, &mut report)?;

    let db_path = expand_db_path_for_auto_fix(db_path)?;
    if let Some(parent) = db_path.parent() {
        fix_path_mode(parent, 0o700, "db parent", &mut report)?;
    }
    fix_path_mode(&db_path, 0o600, "db file", &mut report)?;

    for (label, key_path) in [
        ("swarm key", resolve_swarm_key_path(None, cfg)),
        ("p2p identity key", resolve_p2p_identity_key_path(None, cfg)),
        (
            "relay identity key",
            resolve_relay_identity_key_path(None, cfg),
        ),
    ] {
        let key_path = config::expand_home_path(&key_path)?;
        if let Some(parent) = key_path.parent() {
            fix_path_mode(parent, 0o700, &format!("{label} parent"), &mut report)?;
        }
        fix_path_mode(&key_path, 0o600, label, &mut report)?;
    }

    Ok(report)
}

fn expand_db_path_for_auto_fix(path: &str) -> Result<PathBuf> {
    if path == ":memory:" {
        return Ok(PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var_os("HOME").context("HOME env var not set")?;
        return Ok(Path::new(&home).join(rest));
    }
    Ok(PathBuf::from(path))
}

fn fix_path_mode(
    path: &Path,
    wanted_mode: u32,
    target: &str,
    report: &mut DoctorAutoFixReport,
) -> Result<()> {
    if path == Path::new(":memory:") {
        report.actions.push(DoctorAutoFixAction {
            target: target.to_string(),
            status: DoctorAutoFixStatus::Skipped,
            detail: "path=:memory:".to_string(),
        });
        return Ok(());
    }

    let mode = match file_mode_777(path) {
        Some(mode) => mode,
        None => {
            report.actions.push(DoctorAutoFixAction {
                target: target.to_string(),
                status: DoctorAutoFixStatus::Skipped,
                detail: format!("missing path={}", path.display()),
            });
            return Ok(());
        }
    };

    if mode == wanted_mode {
        report.actions.push(DoctorAutoFixAction {
            target: target.to_string(),
            status: DoctorAutoFixStatus::Ok,
            detail: format!("path={} mode={mode:03o}", path.display()),
        });
        return Ok(());
    }

    set_path_mode(path, wanted_mode)
        .with_context(|| format!("chmod {wanted_mode:03o}: {}", path.display()))?;
    report.actions.push(DoctorAutoFixAction {
        target: target.to_string(),
        status: DoctorAutoFixStatus::Fixed,
        detail: format!("path={} mode={mode:03o}->{wanted_mode:03o}", path.display()),
    });
    Ok(())
}

fn set_path_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

fn fix_default_record_ignore_regex(
    cfg: &config::FileConfig,
    cfg_path: &Path,
    config_error: Option<&str>,
    report: &mut DoctorAutoFixReport,
) -> Result<()> {
    if let Some(error) = config_error {
        report.actions.push(DoctorAutoFixAction {
            target: "record_ignore_regex".to_string(),
            status: DoctorAutoFixStatus::Skipped,
            detail: format!("invalid config: {error}"),
        });
        return Ok(());
    }

    if let Some(pattern) = normalize_opt_string(cfg.record_ignore_regex.clone()) {
        match regex::Regex::new(&pattern) {
            Ok(_) => {
                report.actions.push(DoctorAutoFixAction {
                    target: "record_ignore_regex".to_string(),
                    status: DoctorAutoFixStatus::Ok,
                    detail: "configured".to_string(),
                });
            }
            Err(err) => {
                report.actions.push(DoctorAutoFixAction {
                    target: "record_ignore_regex".to_string(),
                    status: DoctorAutoFixStatus::Skipped,
                    detail: format!("configured pattern is invalid: {err}"),
                });
            }
        }
        return Ok(());
    }

    if let Some(parent) = cfg_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create dir: {}", parent.display()))?;
        set_path_mode(parent, 0o700).with_context(|| format!("chmod 700: {}", parent.display()))?;
    }

    let mut content = match std::fs::read_to_string(cfg_path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(err).with_context(|| format!("read config: {}", cfg_path.display()));
        }
    };
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    if !content.is_empty() {
        content.push('\n');
    }
    content.push_str(&format!(
        "record_ignore_regex = {DEFAULT_RECORD_IGNORE_REGEX:?}\n"
    ));
    std::fs::write(cfg_path, content)
        .with_context(|| format!("write config: {}", cfg_path.display()))?;
    set_path_mode(cfg_path, 0o600).with_context(|| format!("chmod 600: {}", cfg_path.display()))?;
    report.actions.push(DoctorAutoFixAction {
        target: "record_ignore_regex".to_string(),
        status: DoctorAutoFixStatus::Fixed,
        detail: "set default secret-filter pattern".to_string(),
    });
    Ok(())
}

fn print_doctor_auto_fix_report(report: &DoctorAutoFixReport) {
    if report.actions.is_empty() {
        println!("auto-fix: no actions");
        return;
    }

    for action in &report.actions {
        let status = match action.status {
            DoctorAutoFixStatus::Fixed => "fixed",
            DoctorAutoFixStatus::Ok => "ok",
            DoctorAutoFixStatus::Skipped => "skipped",
        };
        println!(
            "auto-fix: {status} target={} {}",
            action.target, action.detail
        );
    }
}

fn build_hook_status_report(cfg: &config::FileConfig) -> DoctorHookStatusReport {
    build_hook_status_report_from_env(
        default_shell(),
        env_nonempty("RUSTORY_HOOK_INSTALLED"),
        env_nonempty("RUSTORY_HOOK_DISABLE"),
        env_nonempty("RUSTORY_SEARCH_LIMIT"),
        cfg.search_limit_default,
    )
}

fn build_hook_status_report_from_env(
    shell: Option<String>,
    installed_marker: Option<String>,
    disable_marker: Option<String>,
    search_limit_raw: Option<String>,
    config_search_limit: Option<usize>,
) -> DoctorHookStatusReport {
    let installed = installed_marker.is_some();
    let (disabled, disable_warning) = resolve_hook_disabled_from_env(disable_marker);
    let (search_limit, error) =
        match resolve_search_limit_from_values(None, search_limit_raw, config_search_limit) {
            Ok(value) => (Some(value), None),
            Err(err) => (None, Some(err.to_string())),
        };

    let mut warnings = Vec::new();
    if !installed {
        let shell_hint = match shell.as_deref() {
            Some("bash" | "zsh") => shell.as_deref().unwrap(),
            _ => "zsh",
        };
        warnings.push(format!(
            "hook marker missing; run source <(rr hook --shell {shell_hint})"
        ));
    }
    if disabled {
        warnings.push("RUSTORY_HOOK_DISABLE is set; record/search hook is disabled".to_string());
    }
    if let Some(warning) = disable_warning {
        warnings.push(warning);
    }
    match shell.as_deref() {
        Some("bash" | "zsh") => {}
        Some(other) => warnings.push(format!("unsupported shell for rr hook: {other}")),
        None => warnings.push("SHELL is not set; choose bash or zsh for rr hook".to_string()),
    }

    DoctorHookStatusReport {
        shell,
        installed,
        disabled,
        search_limit,
        warnings,
        error,
    }
}

fn resolve_hook_disabled_from_env(disable_marker: Option<String>) -> (bool, Option<String>) {
    match disable_marker {
        Some(raw) => match parse_env_bool(&raw, "RUSTORY_HOOK_DISABLE") {
            Ok(value) => (value, None),
            Err(err) => (
                true,
                Some(format!("{err}; disabling record/search hook for safety")),
            ),
        },
        None => (false, None),
    }
}

fn print_hook_status(report: &DoctorHookStatusReport) {
    let shell = report.shell.as_deref().unwrap_or("-");
    let search_limit = report
        .search_limit
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());

    let mut parts = vec![
        format!("installed={}", report.installed),
        format!("disabled={}", report.disabled),
        format!("shell={shell}"),
        format!("search_limit={search_limit}"),
    ];

    if let Some(error) = report.error.as_deref() {
        parts.push(format!("error={error}"));
    }
    if !report.warnings.is_empty() {
        parts.push(format!("warnings={}", report.warnings.join("; ")));
    }

    println!("hook: {}", parts.join(" "));
}

fn print_db_status(report: &DoctorDbStatusReport) {
    if let Some(error) = report.error.as_deref() {
        println!("db status: error: {error}");
        return;
    }

    if !report.exists {
        println!("db status: missing");
        return;
    }

    println!(
        "db status: exists entries={} latest_ingest_seq={} peer_book_peers={} sync_peers={}",
        report.entry_count.unwrap_or(0),
        report.latest_ingest_seq.unwrap_or(0),
        report.peer_book_count.unwrap_or(0),
        report.sync_peer_count.unwrap_or(0),
    );
    if let Some(mode) = report.db_mode {
        let parent = report
            .parent_mode
            .map(|value| format!("{value:03o}"))
            .unwrap_or_else(|| "-".to_string());
        match report.permission_warning.as_deref() {
            Some(warning) => {
                println!("db permissions: db_mode={mode:03o} parent_mode={parent} warn: {warning}")
            }
            None => println!("db permissions: db_mode={mode:03o} parent_mode={parent}"),
        }
    }
}

fn print_tracker_token_status(report: &DoctorTrackerTokenReport) {
    let length = report
        .length
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let mut parts = vec![
        format!("configured={}", report.configured),
        format!("length={length}"),
    ];
    if let Some(warning) = report.warning.as_deref() {
        parts.push(format!("warning={warning}"));
    }
    if let Some(error) = report.error.as_deref() {
        parts.push(format!("error={error}"));
    }

    println!("tracker token: {}", parts.join(" "));
}

fn print_key_status(
    label: &str,
    path: &str,
    value: Option<&str>,
    load_error: Option<&str>,
) -> Result<()> {
    let expanded = config::expand_home_path(path)?;
    let exists = match std::fs::metadata(&expanded) {
        Ok(_) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            println!("{label}: {} (stat error: {err})", expanded.display());
            return Ok(());
        }
    };

    if !exists {
        println!("{label}: {} (missing)", expanded.display());
        return Ok(());
    }

    let mut details = Vec::new();
    if let Some(value) = value {
        details.push(value.to_string());
    }
    if let Some(load_error) = load_error {
        details.push(format!("invalid: {load_error}"));
    }

    if let Some(mode) = file_mode_777(&expanded)
        && mode != 0o600
    {
        details.push(format!("warn: mode={mode:03o}, want 600"));
    }

    if details.is_empty() {
        println!("{label}: {} (exists)", expanded.display());
    } else {
        println!(
            "{label}: {} (exists) {}",
            expanded.display(),
            details.join(" | ")
        );
    }
    Ok(())
}

fn file_mode_777(path: &std::path::Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let md = std::fs::metadata(path).ok()?;
        Some(md.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn tracker_ping(base_url: &str, token: Option<&str>) -> std::result::Result<u64, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(1)))
        .timeout_send_request(Some(Duration::from_secs(1)))
        .timeout_send_body(Some(Duration::from_secs(1)))
        .timeout_recv_response(Some(Duration::from_secs(1)))
        .timeout_recv_body(Some(Duration::from_secs(1)))
        .build()
        .into();

    let url = format!("{}/api/v1/ping", base_url.trim_end_matches('/'));
    let mut req = agent.get(&url);
    if let Some(token) = token {
        req = req.header("Authorization", format!("Bearer {}", token.trim()));
    }

    let started = Instant::now();
    match req.call() {
        Ok(resp) => {
            if resp.status().as_u16() == 200 {
                let elapsed_ms = started.elapsed().as_millis();
                let latency_ms = u64::try_from(elapsed_ms).unwrap_or(u64::MAX);
                Ok(latency_ms)
            } else {
                Err(format!("status {}", resp.status()))
            }
        }
        Err(err) => Err(err.to_string()),
    }
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct SyncStatusPeerReport {
    peer_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer_device_id: Option<String>,
    pull_cursor: i64,
    push_cursor: i64,
    outbound_push_pending: usize,
    pending_push: usize,
    last_seen_unix: Option<i64>,
    last_seen_age_sec: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct SyncStatusTrackerReport {
    base_url: String,
    reachable: bool,
    latency_ms: Option<u64>,
    error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct SyncStatusReport {
    local_head: i64,
    local_device_id: String,
    peers: Vec<SyncStatusPeerReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracker_status: Option<Vec<SyncStatusTrackerReport>>,
}

fn compute_last_seen_age_sec(now_unix: i64, last_seen_unix: Option<i64>) -> Option<i64> {
    last_seen_unix.map(|ts| now_unix.saturating_sub(ts).max(0))
}

fn build_sync_status_report(
    store: &storage::LocalStore,
    local_device_id: &str,
    local_peer_id: Option<&str>,
    peer_filter: Option<&str>,
    tracker_status: Option<Vec<SyncStatusTrackerReport>>,
) -> Result<SyncStatusReport> {
    let local_head = store.latest_ingest_seq()?;
    let peer_last_seen = store.list_peer_book_last_seen_map()?;
    let peer_device_ids = store
        .list_peer_book(None, 0, 1000)?
        .into_iter()
        .filter_map(|peer| peer.device_id.map(|device_id| (peer.peer_id, device_id)))
        .collect::<HashMap<_, _>>();
    let now_unix = time::OffsetDateTime::now_utc().unix_timestamp();
    let mut statuses = store.list_peer_sync_status()?;
    if let Some(peer_id) = peer_filter {
        statuses.retain(|status| status.peer_id == peer_id);
    }

    let mut peers = Vec::with_capacity(statuses.len());
    for status in statuses {
        let peer_id = status.peer_id;
        if local_peer_id == Some(peer_id.as_str()) {
            continue;
        }
        let peer_device_id = peer_device_ids.get(&peer_id).cloned();
        if sync_device_id_matches(peer_device_id.as_deref(), local_device_id) {
            continue;
        }
        let pending_push = store.count_pending_push_entries(&peer_id, Some(local_device_id))?;
        let last_seen_unix = peer_last_seen.get(&peer_id).copied();
        let last_seen_age_sec = compute_last_seen_age_sec(now_unix, last_seen_unix);
        peers.push(SyncStatusPeerReport {
            peer_device_id,
            peer_id,
            pull_cursor: status.last_cursor,
            push_cursor: status.last_pushed_seq,
            outbound_push_pending: pending_push,
            pending_push,
            last_seen_unix,
            last_seen_age_sec,
        });
    }

    Ok(SyncStatusReport {
        local_head,
        local_device_id: local_device_id.to_string(),
        peers,
        tracker_status,
    })
}

fn build_sync_status_report_for_cli(
    store: &storage::LocalStore,
    local_device_id: &str,
    local_peer_id: Option<&str>,
    peer_filter: Option<&str>,
    trackers: Option<&[String]>,
    tracker_token: Option<&str>,
) -> Result<SyncStatusReport> {
    let tracker_status =
        trackers.map(|trackers| build_tracker_status_report(trackers, tracker_token));
    build_sync_status_report(
        store,
        local_device_id,
        local_peer_id,
        peer_filter,
        tracker_status,
    )
}

fn resolve_local_p2p_peer_id(cfg: &config::FileConfig) -> Option<String> {
    let path = resolve_p2p_identity_key_path(None, cfg);
    config::load_identity_keypair(&path)
        .ok()
        .flatten()
        .map(|key| key.public().to_peer_id().to_string())
}

fn sync_device_id_matches(peer_device_id: Option<&str>, local_device_id: &str) -> bool {
    peer_device_id
        .map(|device_id| device_id.trim() == local_device_id.trim())
        .unwrap_or(false)
}

#[derive(Default)]
struct SyncStatusWatchState {
    peers: HashMap<String, SyncStatusWatchPeerState>,
    frame: usize,
}

#[derive(Debug, Clone)]
struct SyncStatusWatchPeerState {
    last_pull_cursor: i64,
    last_push_cursor: i64,
    last_outbound_push_pending: usize,
    max_outbound_push_pending: usize,
    last_sample: Instant,
}

#[derive(Debug, Clone, Copy)]
struct SyncStatusWatchPeerRates {
    pull_per_sec: f64,
    push_per_sec: f64,
    pending_drain_per_sec: f64,
}

#[derive(Debug, Clone)]
struct SyncStatusWatchPeerView<'a> {
    peer: &'a SyncStatusPeerReport,
    rates: SyncStatusWatchPeerRates,
    baseline: usize,
    progress: usize,
    peer_name: String,
}

fn run_sync_status_watch(
    store: &storage::LocalStore,
    local_device_id: &str,
    local_peer_id: Option<&str>,
    peer_filter: Option<&str>,
    trackers: Option<&[String]>,
    tracker_token: Option<&str>,
    interval_sec: u64,
) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc::set_handler(move || {
            stop.store(true, Ordering::SeqCst);
        })
        .context("set Ctrl-C/SIGTERM handler")?;
    }

    let mut state = SyncStatusWatchState::default();
    let mut stdout = io::stdout();
    write!(stdout, "\x1b[?1049h\x1b[?25l")?;
    stdout.flush()?;

    while !stop.load(Ordering::SeqCst) {
        let report = match build_sync_status_report_for_cli(
            store,
            local_device_id,
            local_peer_id,
            peer_filter,
            trackers,
            tracker_token,
        ) {
            Ok(report) => report,
            Err(err) => {
                restore_sync_status_watch_terminal(&mut stdout)?;
                return Err(err);
            }
        };
        let frame_width = sync_status_watch_terminal_size(stdout.as_raw_fd())
            .map(|(width, _height)| width.saturating_sub(1))
            .unwrap_or(150)
            .max(80);
        let frame =
            render_sync_status_watch_frame(&mut state, &report, Instant::now(), frame_width);
        write!(stdout, "\x1b[H\x1b[2J{frame}")?;
        stdout.flush()?;
        sleep_with_stop(Duration::from_secs(interval_sec.max(1)), stop.as_ref());
    }

    restore_sync_status_watch_terminal(&mut stdout)?;
    Ok(())
}

fn restore_sync_status_watch_terminal(stdout: &mut impl Write) -> Result<()> {
    write!(stdout, "\x1b[?25h\x1b[?1049l")?;
    stdout.flush()?;
    Ok(())
}

fn sync_status_watch_terminal_size(fd: RawFd) -> Option<(usize, usize)> {
    let mut size = unsafe { std::mem::zeroed::<libc::winsize>() };
    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut size) } != 0 {
        return None;
    }
    let width = usize::from(size.ws_col);
    let height = usize::from(size.ws_row);
    if width == 0 || height == 0 {
        None
    } else {
        Some((width, height))
    }
}

fn render_sync_status_watch_frame(
    state: &mut SyncStatusWatchState,
    report: &SyncStatusReport,
    now: Instant,
    frame_width: usize,
) -> String {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    const PANEL_GAP: &str = "  ";
    const TRAFFIC_MIN_WIDTH: usize = 44;
    const TRAFFIC_MAX_WIDTH: usize = 62;

    let width = frame_width.max(80);
    let traffic_width = (width / 3).clamp(TRAFFIC_MIN_WIDTH, TRAFFIC_MAX_WIDTH);
    let mesh_width = width.saturating_sub(traffic_width + display_width(PANEL_GAP));

    let spinner = SPINNER[state.frame % SPINNER.len()];
    let frame_index = state.frame;
    state.frame = state.frame.wrapping_add(1);

    let mut out = String::new();
    let total_outbound_pending: usize = report
        .peers
        .iter()
        .map(|peer| peer.outbound_push_pending)
        .sum();
    let mut peer_views = Vec::with_capacity(report.peers.len());
    for peer in &report.peers {
        let rates = sync_status_watch_peer_rates(state, peer, now);
        let baseline = state
            .peers
            .get(&peer.peer_id)
            .map(|state| state.max_outbound_push_pending)
            .unwrap_or(peer.outbound_push_pending);
        let progress = outbound_push_progress_percent(peer.outbound_push_pending, baseline);
        let peer_name = sync_status_peer_display_name(peer);
        peer_views.push(SyncStatusWatchPeerView {
            peer,
            rates,
            baseline,
            progress,
            peer_name,
        });
    }

    push_watch_line(
        &mut out,
        width,
        &format!(
            "{spinner} rustory mesh watch  local={}  head={}  peers={}  outbound={}",
            truncate_display(&report.local_device_id, 24),
            format_count_i64(report.local_head),
            report.peers.len(),
            format_count_usize(total_outbound_pending)
        ),
    );

    out.push('\n');
    let mesh_panel = render_mesh_panel(
        &report.local_device_id,
        &peer_views,
        frame_index,
        mesh_width,
    );
    let traffic_panel = render_traffic_panel(
        report,
        &peer_views,
        report.tracker_status.as_deref(),
        traffic_width,
    );
    for line in join_watch_panels(&mesh_panel, &traffic_panel, PANEL_GAP) {
        push_watch_line(&mut out, width, &line);
    }

    out.push('\n');
    for line in render_link_panel(&peer_views, width) {
        push_watch_line(&mut out, width, &line);
    }

    out.push('\n');
    push_watch_line(
        &mut out,
        width,
        "ctrl+c to exit  •  local observation: real remote peer↔peer flow needs daemon telemetry",
    );
    out
}

fn render_mesh_panel(
    local_device_id: &str,
    peer_views: &[SyncStatusWatchPeerView<'_>],
    frame: usize,
    panel_width: usize,
) -> Vec<String> {
    let inner_width = panel_width.saturating_sub(2);
    let canvas_width = inner_width;
    let canvas_height = 17;
    let mut canvas = WatchCanvas::new(canvas_width, canvas_height);
    let local = CanvasPoint {
        x: canvas_width / 2,
        y: canvas_height / 2,
    };

    let mut nodes = Vec::with_capacity(peer_views.len());
    for (idx, view) in peer_views.iter().enumerate() {
        let label = format!(
            "{} {}",
            mesh_peer_symbol(view),
            truncate_display(&view.peer_name, 20)
        );
        let point = mesh_peer_point(idx, peer_views.len(), canvas_width, canvas_height, &label);
        nodes.push((point, label, view));
    }

    for (idx, (point, _label, view)) in nodes.iter().enumerate() {
        let edge = mesh_edge_glyph(local, *point);
        canvas.draw_line(local, *point, edge);
        if view.peer.outbound_push_pending > 0 || view.rates.push_per_sec > 0.0 {
            let phase = (frame + idx.saturating_mul(2)) % 7 + 1;
            let particle =
                if view.rates.pending_drain_per_sec > 0.0 || view.rates.push_per_sec > 0.0 {
                    '◆'
                } else {
                    '◇'
                };
            canvas.put(point_between(local, *point, phase, 8), particle);
        }
        if view.rates.pull_per_sec > 0.0 {
            let phase = (frame + idx.saturating_mul(3)) % 7 + 1;
            canvas.put(point_between(*point, local, phase, 8), '●');
        }
    }

    for (point, label, _view) in nodes {
        canvas.put_str(point.x, point.y, &label);
    }
    canvas.put_str(
        local.x.saturating_sub(8),
        local.y,
        &format!("◎ {}", truncate_display(local_device_id, 16)),
    );

    box_watch_panel(
        "Mesh Map",
        panel_width,
        canvas
            .into_lines()
            .into_iter()
            .map(|line| truncate_display(line.trim_end(), inner_width))
            .collect(),
    )
}

fn render_traffic_panel(
    report: &SyncStatusReport,
    peer_views: &[SyncStatusWatchPeerView<'_>],
    trackers: Option<&[SyncStatusTrackerReport]>,
    panel_width: usize,
) -> Vec<String> {
    let inner_width = panel_width.saturating_sub(2);
    let total_pending: usize = peer_views
        .iter()
        .map(|view| view.peer.outbound_push_pending)
        .sum();
    let total_baseline: usize = peer_views.iter().map(|view| view.baseline).sum();
    let total_pull_rate: f64 = peer_views.iter().map(|view| view.rates.pull_per_sec).sum();
    let total_push_rate: f64 = peer_views.iter().map(|view| view.rates.push_per_sec).sum();
    let total_drain_rate: f64 = peer_views
        .iter()
        .map(|view| view.rates.pending_drain_per_sec.max(0.0))
        .sum();
    let progress = outbound_push_progress_percent(total_pending, total_baseline);
    let hottest = peer_views
        .iter()
        .max_by_key(|view| view.peer.outbound_push_pending);
    let seen = peer_views
        .iter()
        .filter_map(|view| view.peer.last_seen_age_sec)
        .max()
        .map(|age| format!("oldest {age}s"))
        .unwrap_or_else(|| "unknown".to_string());

    let mut body = vec![
        tracker_summary_line(trackers),
        traffic_kv_line("local", &report.local_device_id, inner_width),
        traffic_kv_line(
            "head",
            &format!(
                "{}   peers {}",
                format_count_i64(report.local_head),
                report.peers.len()
            ),
            inner_width,
        ),
        traffic_kv_line("seen", &seen, inner_width),
        String::new(),
        traffic_rate_line("pull", total_pull_rate, inner_width),
        traffic_rate_line("push", total_push_rate, inner_width),
        traffic_rate_line("drain", total_drain_rate, inner_width),
        traffic_backlog_line(progress, total_pending, inner_width),
    ];

    if let Some(view) = hottest {
        body.push(traffic_hot_line(
            &view.peer_name,
            view.peer.push_cursor,
            view.peer.outbound_push_pending,
            inner_width,
        ));
    }

    body.extend([
        String::new(),
        traffic_kv_line("legend", "◎ local  ● synced", inner_width),
        traffic_kv_line("", "◐ backlog  ○ stale", inner_width),
        traffic_kv_line("flow", "◆ moving  ◇ queued  ● pull", inner_width),
    ]);

    box_watch_panel("Traffic", panel_width, body)
}

fn traffic_kv_line(label: &str, value: &str, inner_width: usize) -> String {
    const LABEL_WIDTH: usize = 8;

    let label = fit_cell(label, LABEL_WIDTH);
    let value_width = inner_width.saturating_sub(LABEL_WIDTH + 1);
    format!("{label} {}", truncate_display(value, value_width))
}

fn traffic_rate_line(label: &str, rate: f64, inner_width: usize) -> String {
    traffic_kv_line(
        label,
        &format!("{}/s", right_cell(&format_rate(rate), 8)),
        inner_width,
    )
}

fn traffic_backlog_line(progress: usize, pending: usize, inner_width: usize) -> String {
    let left = format!(
        "{} [{}]",
        fit_cell("backlog", 8),
        progress_bar(progress, 14)
    );
    let right = format!(
        "{} {} left",
        right_cell(&format!("{progress}%"), 5),
        right_cell(&format_count_usize(pending), 7)
    );
    align_left_right(&left, &right, inner_width)
}

fn traffic_hot_line(
    peer_name: &str,
    push_cursor: i64,
    pending: usize,
    inner_width: usize,
) -> String {
    let right = format!(
        "cur {}  {} left",
        right_cell(&format_count_i64(push_cursor), 7),
        right_cell(&format_count_usize(pending), 7)
    );
    let label_width = 9;
    let peer_width = inner_width
        .saturating_sub(label_width)
        .saturating_sub(display_width(&right))
        .saturating_sub(1);
    let left = format!(
        "{} {}",
        fit_cell("hot", 8),
        truncate_display(peer_name, peer_width)
    );
    align_left_right(&left, &right, inner_width)
}

fn align_left_right(left: &str, right: &str, width: usize) -> String {
    let right_width = display_width(right);
    if width <= right_width {
        return truncate_display(right, width);
    }

    let left_width = width.saturating_sub(right_width + 1);
    format!("{} {}", fit_cell(left, left_width), right)
}

fn render_link_panel(
    peer_views: &[SyncStatusWatchPeerView<'_>],
    panel_width: usize,
) -> Vec<String> {
    let inner_width = panel_width.saturating_sub(2);
    let mut body = if inner_width >= 118 {
        render_link_panel_wide(peer_views, inner_width)
    } else {
        render_link_panel_compact(peer_views, inner_width)
    };

    if peer_views.is_empty() {
        body.push("no peers known yet; run rr daemon or p2p-serve + p2p-sync --push".to_string());
    }

    box_watch_panel("Links", panel_width, body)
}

fn render_link_panel_wide(
    peer_views: &[SyncStatusWatchPeerView<'_>],
    inner_width: usize,
) -> Vec<String> {
    const SEEN_COL: usize = 8;
    const PULL_CURSOR_COL: usize = 10;
    const PULL_RATE_COL: usize = 8;
    const PUSH_CURSOR_COL: usize = 10;
    const PENDING_COL: usize = 10;
    const DRAIN_COL: usize = 8;
    const PROGRESS_COL: usize = 24;
    const COLUMN_GAPS: usize = 7;

    let fixed_width = SEEN_COL
        + PULL_CURSOR_COL
        + PULL_RATE_COL
        + PUSH_CURSOR_COL
        + PENDING_COL
        + DRAIN_COL
        + PROGRESS_COL
        + COLUMN_GAPS;
    let peer_col = inner_width.saturating_sub(fixed_width).max(18);

    let mut body = Vec::new();
    body.push(format!(
        "{} {} {} {} {} {} {} {}",
        fit_cell("peer", peer_col),
        right_cell("seen", SEEN_COL),
        right_cell("pull_cur", PULL_CURSOR_COL),
        right_cell("pull/s", PULL_RATE_COL),
        right_cell("push_cur", PUSH_CURSOR_COL),
        right_cell("pending", PENDING_COL),
        right_cell("drain/s", DRAIN_COL),
        fit_cell("progress", PROGRESS_COL),
    ));
    body.push("─".repeat(inner_width));

    for view in peer_views {
        let peer = view.peer;
        let last_seen = peer
            .last_seen_age_sec
            .map(|age| format!("{age}s"))
            .unwrap_or_else(|| "-".to_string());
        body.push(format!(
            "{} {} {} {} {} {} {} {}",
            fit_cell(&view.peer_name, peer_col),
            right_cell(&last_seen, SEEN_COL),
            right_cell(&format_count_i64(peer.pull_cursor), PULL_CURSOR_COL),
            right_cell(&format_rate(view.rates.pull_per_sec), PULL_RATE_COL),
            right_cell(&format_count_i64(peer.push_cursor), PUSH_CURSOR_COL),
            right_cell(&format_count_usize(peer.outbound_push_pending), PENDING_COL),
            right_cell(
                &format_rate(view.rates.pending_drain_per_sec.max(0.0)),
                DRAIN_COL
            ),
            link_push_progress_line(view.progress, PROGRESS_COL),
        ));
    }

    body
}

fn render_link_panel_compact(
    peer_views: &[SyncStatusWatchPeerView<'_>],
    inner_width: usize,
) -> Vec<String> {
    let seen_col = 7;
    let variable_width = inner_width.saturating_sub(seen_col + 3);
    let peer_col = (variable_width / 4).clamp(18, 28);
    let pull_col = (variable_width / 4).clamp(18, 26);
    let push_col = variable_width.saturating_sub(peer_col + pull_col).max(32);

    let mut body = Vec::new();
    body.push(format!(
        "{} {} {} {}",
        fit_cell("peer", peer_col),
        right_cell("seen", seen_col),
        fit_cell("peer → local", pull_col),
        fit_cell("local → peer", push_col)
    ));
    body.push("─".repeat(inner_width));

    for view in peer_views {
        let peer = view.peer;
        let last_seen = peer
            .last_seen_age_sec
            .map(|age| format!("{age}s"))
            .unwrap_or_else(|| "-".to_string());
        let pull = format!(
            "cur {} {}/s",
            format_count_i64(peer.pull_cursor),
            format_rate(view.rates.pull_per_sec)
        );
        let push = format!(
            "cur {} pend {} {}",
            format_count_i64(peer.push_cursor),
            format_count_usize(peer.outbound_push_pending),
            link_push_progress_line(view.progress, 12),
        );
        body.push(format!(
            "{} {} {} {}",
            fit_cell(&view.peer_name, peer_col),
            right_cell(&last_seen, seen_col),
            fit_cell(&pull, pull_col),
            fit_cell(&push, push_col),
        ));
    }

    body
}

fn link_push_progress_line(progress: usize, width: usize) -> String {
    if width < 8 {
        return truncate_display(&format!("{progress}%"), width);
    }
    let bar_width = width.saturating_sub(8).clamp(4, 18);
    let left = format!("[{}]", progress_bar(progress, bar_width));
    let right = right_cell(&format!("{progress}%"), 5);
    align_left_right(&left, &right, width)
}

fn tracker_summary_line(trackers: Option<&[SyncStatusTrackerReport]>) -> String {
    let Some(trackers) = trackers else {
        return "tracker not checked".to_string();
    };
    if trackers.is_empty() {
        return "tracker none configured".to_string();
    }

    let reachable = trackers.iter().filter(|tracker| tracker.reachable).count();
    let latency = trackers
        .iter()
        .filter_map(|tracker| tracker.latency_ms)
        .min()
        .map(|latency| format!("{latency}ms"))
        .unwrap_or_else(|| "-".to_string());
    let first = trackers
        .first()
        .map(|tracker| tracker.base_url.as_str())
        .unwrap_or("-");
    format!(
        "tracker {reachable}/{} {latency} {}",
        trackers.len(),
        truncate_display(first, 24)
    )
}

fn mesh_peer_symbol(view: &SyncStatusWatchPeerView<'_>) -> char {
    if view.peer.last_seen_age_sec.is_some_and(|age| age > 300) {
        '○'
    } else if view.peer.outbound_push_pending > 0 {
        '◐'
    } else if view.rates.pull_per_sec > 0.0
        || view.rates.push_per_sec > 0.0
        || view.rates.pending_drain_per_sec > 0.0
    {
        '◉'
    } else {
        '●'
    }
}

fn mesh_peer_point(
    index: usize,
    total: usize,
    width: usize,
    height: usize,
    label: &str,
) -> CanvasPoint {
    if total == 0 {
        return CanvasPoint { x: 1, y: 1 };
    }
    let center_x = width as f64 / 2.0;
    let center_y = height as f64 / 2.0;
    let radius_x = (width.saturating_sub(22) as f64 / 2.0).max(8.0);
    let radius_y = (height.saturating_sub(4) as f64 / 2.0).max(3.0);
    let angle =
        -std::f64::consts::FRAC_PI_2 + (index as f64 * std::f64::consts::TAU / total as f64);
    let label_width = unicode_width::UnicodeWidthStr::width(label);
    let max_x = width.saturating_sub(label_width.saturating_add(1)).max(1);
    let max_y = height.saturating_sub(2).max(1);
    let x = (center_x + radius_x * angle.cos()).round();
    let y = (center_y + radius_y * angle.sin()).round();
    CanvasPoint {
        x: (x as isize).clamp(1, max_x as isize) as usize,
        y: (y as isize).clamp(1, max_y as isize) as usize,
    }
}

fn mesh_edge_glyph(from: CanvasPoint, to: CanvasPoint) -> char {
    let dx = to.x as isize - from.x as isize;
    let dy = to.y as isize - from.y as isize;
    if dx.abs() > dy.abs().saturating_mul(2) {
        '─'
    } else if dy.abs() > dx.abs().saturating_mul(2) {
        '│'
    } else if dx.signum() == dy.signum() {
        '╲'
    } else {
        '╱'
    }
}

fn point_between(from: CanvasPoint, to: CanvasPoint, step: usize, total: usize) -> CanvasPoint {
    let total = total.max(1) as f64;
    let t = step as f64 / total;
    CanvasPoint {
        x: (from.x as f64 + (to.x as f64 - from.x as f64) * t).round() as usize,
        y: (from.y as f64 + (to.y as f64 - from.y as f64) * t).round() as usize,
    }
}

fn join_watch_panels(left: &[String], right: &[String], gap: &str) -> Vec<String> {
    let left_width = left
        .first()
        .map(|line| unicode_width::UnicodeWidthStr::width(line.as_str()))
        .unwrap_or(0);
    let right_width = right
        .first()
        .map(|line| unicode_width::UnicodeWidthStr::width(line.as_str()))
        .unwrap_or(0);
    let height = left.len().max(right.len());
    let mut lines = Vec::with_capacity(height);
    for idx in 0..height {
        let left_line = left
            .get(idx)
            .cloned()
            .unwrap_or_else(|| " ".repeat(left_width));
        let right_line = right
            .get(idx)
            .cloned()
            .unwrap_or_else(|| " ".repeat(right_width));
        lines.push(format!(
            "{}{}{}",
            fit_cell(&left_line, left_width),
            gap,
            fit_cell(&right_line, right_width)
        ));
    }
    lines
}

fn box_watch_panel(title: &str, width: usize, body: Vec<String>) -> Vec<String> {
    let width = width.max(4);
    let inner = width.saturating_sub(2);
    let title = format!(" {title} ");
    let title_width = unicode_width::UnicodeWidthStr::width(title.as_str());
    let top_fill = inner.saturating_sub(title_width);
    let mut lines = Vec::with_capacity(body.len() + 2);
    lines.push(format!("┌{title}{}┐", "─".repeat(top_fill)));
    for line in body {
        lines.push(format!("│{}│", fit_cell(&line, inner)));
    }
    lines.push(format!("└{}┘", "─".repeat(inner)));
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CanvasPoint {
    x: usize,
    y: usize,
}

struct WatchCanvas {
    width: usize,
    height: usize,
    cells: Vec<Vec<char>>,
}

impl WatchCanvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![vec![' '; width]; height],
        }
    }

    fn put(&mut self, point: CanvasPoint, ch: char) {
        if point.y < self.height && point.x < self.width {
            self.cells[point.y][point.x] = ch;
        }
    }

    fn put_str(&mut self, x: usize, y: usize, value: &str) {
        if y >= self.height || x >= self.width {
            return;
        }
        let mut x = x;
        for ch in value.chars() {
            if x >= self.width {
                break;
            }
            self.cells[y][x] = ch;
            x = x.saturating_add(
                unicode_width::UnicodeWidthChar::width(ch)
                    .unwrap_or(1)
                    .max(1),
            );
        }
    }

    fn draw_line(&mut self, from: CanvasPoint, to: CanvasPoint, ch: char) {
        let mut x0 = from.x as isize;
        let mut y0 = from.y as isize;
        let x1 = to.x as isize;
        let y1 = to.y as isize;
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            let point = CanvasPoint {
                x: x0.max(0) as usize,
                y: y0.max(0) as usize,
            };
            if point != from && point != to {
                self.put(point, ch);
            }
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = err.saturating_mul(2);
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn into_lines(self) -> Vec<String> {
        self.cells
            .into_iter()
            .map(|row| row.into_iter().collect::<String>())
            .collect()
    }
}

fn sync_status_watch_peer_rates(
    state: &mut SyncStatusWatchState,
    peer: &SyncStatusPeerReport,
    now: Instant,
) -> SyncStatusWatchPeerRates {
    let Some(previous) = state.peers.get_mut(&peer.peer_id) else {
        state.peers.insert(
            peer.peer_id.clone(),
            SyncStatusWatchPeerState {
                last_pull_cursor: peer.pull_cursor,
                last_push_cursor: peer.push_cursor,
                last_outbound_push_pending: peer.outbound_push_pending,
                max_outbound_push_pending: peer.outbound_push_pending,
                last_sample: now,
            },
        );
        return SyncStatusWatchPeerRates {
            pull_per_sec: 0.0,
            push_per_sec: 0.0,
            pending_drain_per_sec: 0.0,
        };
    };

    let elapsed = now
        .duration_since(previous.last_sample)
        .as_secs_f64()
        .max(0.001);
    let pull_per_sec = peer
        .pull_cursor
        .saturating_sub(previous.last_pull_cursor)
        .max(0) as f64
        / elapsed;
    let push_per_sec = peer
        .push_cursor
        .saturating_sub(previous.last_push_cursor)
        .max(0) as f64
        / elapsed;
    let pending_drain_per_sec =
        previous.last_outbound_push_pending as f64 - peer.outbound_push_pending as f64;
    let pending_drain_per_sec = pending_drain_per_sec / elapsed;

    previous.last_pull_cursor = peer.pull_cursor;
    previous.last_push_cursor = peer.push_cursor;
    previous.last_outbound_push_pending = peer.outbound_push_pending;
    previous.max_outbound_push_pending = previous
        .max_outbound_push_pending
        .max(peer.outbound_push_pending);
    previous.last_sample = now;

    SyncStatusWatchPeerRates {
        pull_per_sec,
        push_per_sec,
        pending_drain_per_sec,
    }
}

fn sync_status_peer_display_name(peer: &SyncStatusPeerReport) -> String {
    let peer_id = short_peer_id(&peer.peer_id);
    if let Some(device_id) = peer.peer_device_id.as_deref() {
        format!("{device_id} {peer_id}")
    } else {
        peer_id
    }
}

fn short_peer_id(peer_id: &str) -> String {
    let prefix = peer_id.chars().take(10).collect::<String>();
    if peer_id.chars().count() <= 10 {
        prefix
    } else {
        format!("{prefix}…")
    }
}

fn outbound_push_progress_percent(pending: usize, baseline: usize) -> usize {
    if pending == 0 {
        return 100;
    }
    if baseline == 0 {
        return 0;
    }
    baseline.saturating_sub(pending).saturating_mul(100) / baseline
}

fn progress_bar(percent: usize, width: usize) -> String {
    let filled = percent.min(100).saturating_mul(width) / 100;
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

fn format_rate(value: f64) -> String {
    if value.abs() >= 1000.0 {
        format!("{:.1}k", value / 1000.0)
    } else {
        format!("{value:.0}")
    }
}

fn format_count_i64(value: i64) -> String {
    if value < 0 {
        return value.to_string();
    }
    format_count_usize(usize::try_from(value).unwrap_or(usize::MAX))
}

fn format_count_usize(value: usize) -> String {
    if value >= 1_000_000_000 {
        format!("{:.1}B", value as f64 / 1_000_000_000.0)
    } else if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn push_watch_line(out: &mut String, width: usize, line: &str) {
    out.push_str(&truncate_display(line, width));
    out.push('\n');
}

fn display_width(value: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(value)
}

fn fit_cell(value: &str, width: usize) -> String {
    let truncated = truncate_display(value, width);
    let current = display_width(truncated.as_str());
    if current >= width {
        truncated
    } else {
        format!("{truncated}{}", " ".repeat(width - current))
    }
}

fn right_cell(value: &str, width: usize) -> String {
    let truncated = truncate_display(value, width);
    let current = display_width(truncated.as_str());
    if current >= width {
        truncated
    } else {
        format!("{}{}", " ".repeat(width - current), truncated)
    }
}

fn truncate_display(value: &str, max_width: usize) -> String {
    if unicode_width::UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let mut out = String::new();
    let mut width = 0;
    let ellipsis_width = 1;
    for ch in value.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width + ellipsis_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push('…');
    out
}

fn build_tracker_status_report(
    trackers: &[String],
    tracker_token: Option<&str>,
) -> Vec<SyncStatusTrackerReport> {
    trackers
        .iter()
        .map(|base_url| match tracker_ping(base_url, tracker_token) {
            Ok(latency_ms) => SyncStatusTrackerReport {
                base_url: base_url.clone(),
                reachable: true,
                latency_ms: Some(latency_ms),
                error: None,
            },
            Err(err) => SyncStatusTrackerReport {
                base_url: base_url.clone(),
                reachable: false,
                latency_ms: None,
                error: Some(err),
            },
        })
        .collect()
}

fn default_cwd() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| ".".to_string())
}

fn default_shell() -> Option<String> {
    let shell = std::env::var("SHELL").ok()?;
    let name = std::path::Path::new(&shell)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())?;
    if name.is_empty() { None } else { Some(name) }
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

fn env_nonempty(key: &str) -> Option<String> {
    normalize_opt_string(std::env::var(key).ok())
}

fn maybe_spawn_async_upload(db_path: &str, cfg: &config::FileConfig) -> Result<()> {
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

fn maybe_run_auto_prune(store: &storage::LocalStore, cfg: &config::FileConfig) -> Result<()> {
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

fn resolve_bool_setting(
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

fn resolve_u64_setting(
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

fn resolve_usize_setting(
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

fn resolve_string_setting(
    env_value: Option<String>,
    cfg_value: Option<String>,
    default: &str,
) -> String {
    env_value
        .or_else(|| normalize_opt_string(cfg_value))
        .unwrap_or_else(|| default.to_string())
}

fn parse_env_bool(value: &str, label: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => {
            anyhow::bail!("invalid {label}={value:?}; expected one of 1/0/true/false/yes/no/on/off")
        }
    }
}

fn read_rate_limit_marker(path: &std::path::Path) -> Result<Option<i64>> {
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

fn write_rate_limit_marker(path: &std::path::Path, now_unix: i64) -> Result<()> {
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

fn should_trigger_interval(
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

fn resolve_search_limit(cli: Option<usize>, cfg: &config::FileConfig) -> Result<usize> {
    resolve_search_limit_from_values(
        cli,
        env_nonempty("RUSTORY_SEARCH_LIMIT"),
        cfg.search_limit_default,
    )
}

fn resolve_search_limit_from_values(
    cli: Option<usize>,
    env_value: Option<String>,
    config_value: Option<usize>,
) -> Result<usize> {
    if let Some(v) = cli {
        return Ok(v);
    }

    if let Some(v) = env_value {
        let parsed: usize = v
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid RUSTORY_SEARCH_LIMIT={:?}: {e}", v.trim()))?;
        return Ok(parsed);
    }

    if let Some(v) = config_value {
        return Ok(v);
    }

    Ok(DEFAULT_HOOK_SEARCH_LIMIT)
}

fn compute_prune_cutoff_unix(now_unix: i64, older_than_days: u64) -> Result<i64> {
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

fn resolve_p2p_watch_start_jitter_sec(cli: Option<u64>, cfg: &config::FileConfig) -> Result<u64> {
    if let Some(v) = cli {
        return Ok(v);
    }

    if let Some(v) = env_nonempty("RUSTORY_P2P_WATCH_START_JITTER_SEC") {
        let parsed: u64 = v.parse().map_err(|e| {
            anyhow::anyhow!(
                "invalid RUSTORY_P2P_WATCH_START_JITTER_SEC={:?}: {e}",
                v.trim()
            )
        })?;
        return Ok(parsed);
    }

    if let Some(v) = cfg.p2p_watch_start_jitter_sec {
        return Ok(v);
    }

    Ok(0)
}

fn resolve_p2p_request_retry_policy(
    cli_attempts: Option<u64>,
    cli_timeout_base_sec: Option<u64>,
    cli_timeout_cap_sec: Option<u64>,
    cli_backoff_base_ms: Option<u64>,
    cfg: &config::FileConfig,
) -> Result<p2p::RequestRetryPolicy> {
    let mut out = p2p::RequestRetryPolicy::default();

    if let Some(v) = cli_attempts {
        out.attempts = parse_attempts(v, "req-attempts")?;
    } else if let Some(v) = env_nonempty("RUSTORY_P2P_REQUEST_ATTEMPTS") {
        let parsed: u64 = v.parse().map_err(|e| {
            anyhow::anyhow!("invalid RUSTORY_P2P_REQUEST_ATTEMPTS={:?}: {e}", v.trim())
        })?;
        out.attempts = parse_attempts(parsed, "RUSTORY_P2P_REQUEST_ATTEMPTS")?;
    } else if let Some(v) = cfg.p2p_request_attempts {
        out.attempts = parse_attempts(v, "p2p_request_attempts")?;
    }

    if let Some(v) = cli_timeout_base_sec {
        out.timeout_base = Duration::from_secs(v);
    } else if let Some(v) = env_nonempty("RUSTORY_P2P_REQUEST_TIMEOUT_BASE_SEC") {
        let parsed: u64 = v.parse().map_err(|e| {
            anyhow::anyhow!(
                "invalid RUSTORY_P2P_REQUEST_TIMEOUT_BASE_SEC={:?}: {e}",
                v.trim()
            )
        })?;
        out.timeout_base = Duration::from_secs(parsed);
    } else if let Some(v) = cfg.p2p_request_timeout_base_sec {
        out.timeout_base = Duration::from_secs(v);
    }

    if let Some(v) = cli_timeout_cap_sec {
        out.timeout_cap = Duration::from_secs(v);
    } else if let Some(v) = env_nonempty("RUSTORY_P2P_REQUEST_TIMEOUT_CAP_SEC") {
        let parsed: u64 = v.parse().map_err(|e| {
            anyhow::anyhow!(
                "invalid RUSTORY_P2P_REQUEST_TIMEOUT_CAP_SEC={:?}: {e}",
                v.trim()
            )
        })?;
        out.timeout_cap = Duration::from_secs(parsed);
    } else if let Some(v) = cfg.p2p_request_timeout_cap_sec {
        out.timeout_cap = Duration::from_secs(v);
    }

    if out.timeout_cap < out.timeout_base {
        out.timeout_cap = out.timeout_base;
    }

    if let Some(v) = cli_backoff_base_ms {
        out.backoff_base = Duration::from_millis(v);
    } else if let Some(v) = env_nonempty("RUSTORY_P2P_REQUEST_BACKOFF_BASE_MS") {
        let parsed: u64 = v.parse().map_err(|e| {
            anyhow::anyhow!(
                "invalid RUSTORY_P2P_REQUEST_BACKOFF_BASE_MS={:?}: {e}",
                v.trim()
            )
        })?;
        out.backoff_base = Duration::from_millis(parsed);
    } else if let Some(v) = cfg.p2p_request_backoff_base_ms {
        out.backoff_base = Duration::from_millis(v);
    }

    Ok(out)
}

fn parse_attempts(value: u64, label: &str) -> Result<usize> {
    if value == 0 {
        anyhow::bail!("{label} must be >= 1");
    }

    usize::try_from(value).map_err(|_| anyhow::anyhow!("{label} is too large"))
}

fn resolve_swarm_psk(
    cli_path: Option<String>,
    cfg: &config::FileConfig,
) -> Result<libp2p::pnet::PreSharedKey> {
    let path = resolve_swarm_key_path(cli_path, cfg);
    config::load_or_generate_swarm_key(&path)
}

fn resolve_swarm_key_path(cli_path: Option<String>, cfg: &config::FileConfig) -> String {
    normalize_opt_string(cli_path)
        .or_else(|| env_nonempty("RUSTORY_SWARM_KEY_PATH"))
        .or_else(|| normalize_opt_string(cfg.swarm_key_path.clone()))
        .unwrap_or_else(|| config::DEFAULT_SWARM_KEY_PATH.to_string())
}

fn resolve_p2p_identity(
    cli_path: Option<String>,
    cfg: &config::FileConfig,
) -> Result<libp2p::identity::Keypair> {
    let path = resolve_p2p_identity_key_path(cli_path, cfg);
    config::load_or_generate_identity_keypair(&path)
}

fn resolve_p2p_identity_key_path(cli_path: Option<String>, cfg: &config::FileConfig) -> String {
    normalize_opt_string(cli_path)
        .or_else(|| env_nonempty("RUSTORY_P2P_IDENTITY_KEY_PATH"))
        .or_else(|| normalize_opt_string(cfg.p2p_identity_key_path.clone()))
        .unwrap_or_else(|| config::DEFAULT_P2P_IDENTITY_KEY_PATH.to_string())
}

fn resolve_relay_identity(
    cli_path: Option<String>,
    cfg: &config::FileConfig,
) -> Result<libp2p::identity::Keypair> {
    let path = resolve_relay_identity_key_path(cli_path, cfg);
    config::load_or_generate_identity_keypair(&path)
}

fn resolve_relay_identity_key_path(cli_path: Option<String>, cfg: &config::FileConfig) -> String {
    normalize_opt_string(cli_path)
        .or_else(|| env_nonempty("RUSTORY_RELAY_IDENTITY_KEY_PATH"))
        .or_else(|| normalize_opt_string(cfg.relay_identity_key_path.clone()))
        .unwrap_or_else(|| config::DEFAULT_RELAY_IDENTITY_KEY_PATH.to_string())
}

fn resolve_relay_addr(
    cli: Option<String>,
    cfg: &config::FileConfig,
) -> Result<Option<libp2p::Multiaddr>> {
    let raw = normalize_opt_string(cli)
        .or_else(|| env_nonempty("RUSTORY_RELAY_ADDR"))
        .or_else(|| normalize_opt_string(cfg.relay_addr.clone()));

    let Some(raw) = raw else {
        return Ok(None);
    };
    Ok(Some(raw.parse().context("parse relay_addr")?))
}

fn relay_addr_reachability_warning(addr: &libp2p::Multiaddr) -> Option<String> {
    for protocol in addr.iter() {
        match protocol {
            Protocol::Ip4(ip) => {
                if is_ipv4_shared_space(ip) {
                    return Some(
                        "uses 100.64.0.0/10 shared address space; peers outside that tailnet/CGNAT path cannot dial this relay"
                            .to_string(),
                    );
                }
                if ip.is_loopback() {
                    return Some(
                        "uses loopback address; only local processes can dial this relay"
                            .to_string(),
                    );
                }
                if ip.is_private() {
                    return Some(
                        "uses private RFC1918 address; internet peers need VPN, split-DNS, or router forwarding"
                            .to_string(),
                    );
                }
                if ip.is_link_local() {
                    return Some(
                        "uses link-local address; off-link peers cannot dial this relay"
                            .to_string(),
                    );
                }
                if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
                    return Some("uses a non-dialable IPv4 address".to_string());
                }
                return None;
            }
            Protocol::Ip6(ip) => {
                if ip.is_loopback() {
                    return Some(
                        "uses loopback address; only local processes can dial this relay"
                            .to_string(),
                    );
                }
                if ip.is_unique_local() {
                    return Some(
                        "uses private IPv6 unique-local address; internet peers need VPN, split-DNS, or router forwarding"
                            .to_string(),
                    );
                }
                if ip.is_unicast_link_local() {
                    return Some(
                        "uses link-local address; off-link peers cannot dial this relay"
                            .to_string(),
                    );
                }
                if ip.is_unspecified() || ip.is_multicast() {
                    return Some("uses a non-dialable IPv6 address".to_string());
                }
                return None;
            }
            Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) | Protocol::Dnsaddr(_) => {
                return None;
            }
            _ => {}
        }
    }

    None
}

fn is_ipv4_shared_space(ip: std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn resolve_trackers(cli: Vec<String>, cfg: &config::FileConfig) -> Result<Vec<String>> {
    let raw_list = if !cli.is_empty() {
        cli
    } else if let Some(env) = env_nonempty("RUSTORY_TRACKERS") {
        env.split(',').map(|s| s.to_string()).collect()
    } else {
        cfg.trackers.clone().unwrap_or_default()
    };

    Ok(raw_list
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

fn resolve_tracker_token_raw(cli: Option<String>, cfg: &config::FileConfig) -> Option<String> {
    normalize_opt_string(cli)
        .or_else(|| env_nonempty("RUSTORY_TRACKER_TOKEN"))
        .or_else(|| normalize_opt_string(cfg.tracker_token.clone()))
}

fn validate_resolved_tracker_token(token: Option<String>) -> Result<Option<String>> {
    if let Some(token) = token.as_deref() {
        tracker::validate_tracker_token_value(token, "tracker token")?;
    }

    Ok(token)
}

fn resolve_tracker_token(cli: Option<String>, cfg: &config::FileConfig) -> Result<Option<String>> {
    validate_resolved_tracker_token(resolve_tracker_token_raw(cli, cfg))
}

fn resolve_peer_meta(cfg: &config::FileConfig) -> crate::tracker::PeerMeta {
    let hostname = env_nonempty("HOSTNAME").unwrap_or_else(|| "unknown".to_string());
    let user_id = resolve_user_id(cfg);
    let device_id = resolve_device_id(cfg);

    crate::tracker::PeerMeta {
        device_id: Some(device_id),
        hostname: Some(hostname),
        user_id: Some(user_id),
        version: Some(crate::build_info::VERSION.to_string()),
        build_revision: Some(crate::build_info::BUILD_REVISION.to_string()),
        build_dirty: Some(crate::build_info::build_dirty()),
    }
}

fn resolve_user_id(cfg: &config::FileConfig) -> String {
    env_nonempty("RUSTORY_USER_ID")
        .or_else(|| normalize_opt_string(cfg.user_id.clone()))
        .or_else(|| env_nonempty("USER"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn resolve_device_id(cfg: &config::FileConfig) -> String {
    env_nonempty("RUSTORY_DEVICE_ID")
        .or_else(|| normalize_opt_string(cfg.device_id.clone()))
        .unwrap_or_else(|| {
            env_nonempty("HOSTNAME")
                .or_else(|| env_nonempty("HOST"))
                .unwrap_or_else(|| "unknown".to_string())
        })
}

fn resolve_record_ignore_regex(cfg: &config::FileConfig) -> Option<String> {
    env_nonempty("RUSTORY_RECORD_IGNORE_REGEX")
        .or_else(|| normalize_opt_string(cfg.record_ignore_regex.clone()))
}

fn resolve_http_sync_token(cli: Option<String>) -> Option<String> {
    normalize_opt_string(cli).or_else(|| env_nonempty("RUSTORY_HTTP_SYNC_TOKEN"))
}

fn validate_http_sync_serve_auth(
    bind: &str,
    token: Option<&str>,
    allow_unauthenticated: bool,
) -> Result<()> {
    if token.is_some() || allow_unauthenticated || is_loopback_bind(bind) {
        return Ok(());
    }

    anyhow::bail!(
        "refusing to serve unauthenticated HTTP sync API on non-loopback bind address {bind}; pass --token or --allow-unauthenticated"
    );
}

fn validate_tracker_serve_auth(
    bind: &str,
    token: Option<&str>,
    allow_unauthenticated: bool,
) -> Result<()> {
    if token.is_some() || allow_unauthenticated || is_loopback_bind(bind) {
        return Ok(());
    }

    anyhow::bail!(
        "refusing to serve unauthenticated tracker on non-loopback bind address {bind}; pass --token or --allow-unauthenticated"
    );
}

fn is_loopback_bind(bind: &str) -> bool {
    if let Ok(addr) = bind.parse::<std::net::SocketAddr>() {
        return addr.ip().is_loopback();
    }

    bind.starts_with("localhost:") || bind.starts_with("[::1]:")
}

fn should_ignore_record_command(
    cmd: &str,
    pattern: &str,
) -> std::result::Result<bool, regex::Error> {
    let re = regex::Regex::new(pattern)?;
    Ok(re.is_match(cmd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_help_describes_key_commands() {
        use clap::CommandFactory;

        let mut cmd = App::command();
        let help = cmd.render_help().to_string();

        assert!(help.contains("Write config and create local P2P key files"));
        assert!(help.contains("Diagnose local config, tools, keys, and connectivity"));
        assert!(help.contains("Run p2p-serve plus p2p-sync watch as one supervised process"));
        assert!(help.contains("Sync with P2P peers, trackers, or cached peers"));
        assert!(help.contains("Self-update the rr binary from release assets"));
        assert!(help.contains("Print a bash or zsh shell hook"));
        assert!(help.contains("Path to the local SQLite history database"));
    }

    #[test]
    fn init_help_describes_init_purpose() {
        use clap::CommandFactory;

        let mut cmd = App::command();
        let init = cmd.find_subcommand_mut("init").expect("init subcommand");
        let help = init.render_help().to_string();

        assert!(help.contains("Write config and create local P2P key files"));
        assert!(help.contains("Overwrite an existing config.toml"));
        assert!(help.contains("Comma-separated tracker base URLs to write into config"));
    }

    #[test]
    fn cli_help_describes_all_visible_options() {
        use clap::CommandFactory;

        let cmd = App::command();
        assert_args_have_help(&cmd);
    }

    fn assert_args_have_help(cmd: &clap::Command) {
        for arg in cmd.get_arguments() {
            assert!(
                arg.get_help().is_some() || arg.get_long_help().is_some(),
                "{} option '{}' has no help text",
                cmd.get_name(),
                arg.get_id()
            );
        }

        for subcommand in cmd.get_subcommands() {
            assert_args_have_help(subcommand);
        }
    }

    #[test]
    fn flag_help_describes_onboarding_options() {
        use clap::CommandFactory;

        let mut cmd = App::command();

        let doctor = cmd
            .find_subcommand_mut("doctor")
            .expect("doctor subcommand");
        let doctor_help = doctor.render_help().to_string();
        assert!(doctor_help.contains("Print diagnostics as pretty JSON"));

        let p2p_sync = cmd
            .find_subcommand_mut("p2p-sync")
            .expect("p2p-sync subcommand");
        let p2p_sync_help = p2p_sync.render_help().to_string();
        assert!(p2p_sync_help.contains("Comma-separated peer multiaddrs to dial directly"));
        assert!(p2p_sync_help.contains("Run sync repeatedly until interrupted"));
        assert!(p2p_sync_help.contains("Relay multiaddr preferred for tracker-discovered peers"));

        let record = cmd
            .find_subcommand_mut("record")
            .expect("record subcommand");
        let record_help = record.render_help().to_string();
        assert!(record_help.contains("Shell command line to record"));
        assert!(record_help.contains("Print the inserted entry id"));

        let relay_serve = cmd
            .find_subcommand_mut("relay-serve")
            .expect("relay-serve subcommand");
        let relay_serve_help = relay_serve.render_help().to_string();
        assert!(relay_serve_help.contains("Maximum active relay circuits"));
        assert!(relay_serve_help.contains("Maximum bytes transferred by a relay circuit"));
    }

    #[test]
    fn relay_serve_parses_capacity_flags() {
        let app = App::parse_from([
            "rr",
            "relay-serve",
            "--max-reservations",
            "1024",
            "--max-reservations-per-peer",
            "128",
            "--max-circuits",
            "512",
            "--max-circuits-per-peer",
            "128",
            "--max-circuit-duration-sec",
            "1200",
            "--max-circuit-bytes",
            "134217728",
            "--rate-limits",
        ]);
        match app.cmd {
            Command::RelayServe {
                max_reservations,
                max_reservations_per_peer,
                max_circuits,
                max_circuits_per_peer,
                max_circuit_duration_sec,
                max_circuit_bytes,
                rate_limits,
                ..
            } => {
                assert_eq!(max_reservations, 1024);
                assert_eq!(max_reservations_per_peer, 128);
                assert_eq!(max_circuits, 512);
                assert_eq!(max_circuits_per_peer, 128);
                assert_eq!(max_circuit_duration_sec, 1200);
                assert_eq!(max_circuit_bytes, 134217728);
                assert!(rate_limits);
            }
            _ => panic!("expected relay-serve"),
        }
    }

    #[test]
    fn relay_addr_warning_flags_tailnet_and_private_addresses() {
        let tailnet: libp2p::Multiaddr = "/ip4/100.64.0.1/tcp/4001"
            .parse()
            .expect("tailnet relay addr");
        let private: libp2p::Multiaddr = "/ip4/192.168.1.10/tcp/4001"
            .parse()
            .expect("private relay addr");
        let dns: libp2p::Multiaddr = "/dns4/rustory-relay.example.com/tcp/4001"
            .parse()
            .expect("dns relay addr");

        assert!(
            relay_addr_reachability_warning(&tailnet)
                .expect("tailnet warning")
                .contains("100.64.0.0/10")
        );
        assert!(
            relay_addr_reachability_warning(&private)
                .expect("private warning")
                .contains("RFC1918")
        );
        assert_eq!(relay_addr_reachability_warning(&dns), None);
    }

    #[test]
    fn p2p_sync_watch_parses_flags() {
        let app = App::parse_from([
            "rr",
            "p2p-sync",
            "--watch",
            "--interval-sec",
            "5",
            "--max-peers-per-tick",
            "2",
        ]);
        match app.cmd {
            Command::P2pSync {
                watch,
                interval_sec,
                start_jitter_sec,
                max_peers_per_tick,
                ..
            } => {
                assert!(watch);
                assert_eq!(interval_sec, 5);
                assert!(start_jitter_sec.is_none());
                assert_eq!(max_peers_per_tick, 2);
            }
            _ => panic!("expected p2p-sync"),
        }
    }

    #[test]
    fn p2p_sync_watch_parses_start_jitter() {
        let app = App::parse_from([
            "rr",
            "p2p-sync",
            "--watch",
            "--interval-sec",
            "5",
            "--start-jitter-sec",
            "3",
        ]);
        match app.cmd {
            Command::P2pSync {
                watch,
                interval_sec,
                start_jitter_sec,
                ..
            } => {
                assert!(watch);
                assert_eq!(interval_sec, 5);
                assert_eq!(start_jitter_sec, Some(3));
            }
            _ => panic!("expected p2p-sync"),
        }
    }

    #[test]
    fn p2p_sync_parses_request_retry_flags() {
        let app = App::parse_from([
            "rr",
            "p2p-sync",
            "--limit",
            "10",
            "--req-attempts",
            "4",
            "--req-timeout-base-sec",
            "7",
            "--req-timeout-cap-sec",
            "33",
            "--req-backoff-base-ms",
            "250",
        ]);

        match app.cmd {
            Command::P2pSync {
                limit,
                req_attempts,
                req_timeout_base_sec,
                req_timeout_cap_sec,
                req_backoff_base_ms,
                ..
            } => {
                assert_eq!(limit, 10);
                assert_eq!(req_attempts, Some(4));
                assert_eq!(req_timeout_base_sec, Some(7));
                assert_eq!(req_timeout_cap_sec, Some(33));
                assert_eq!(req_backoff_base_ms, Some(250));
            }
            _ => panic!("expected p2p-sync"),
        }
    }

    #[test]
    fn tracker_serve_auth_guard_rejects_public_no_token() {
        let err = validate_tracker_serve_auth("0.0.0.0:8850", None, false).unwrap_err();
        assert!(format!("{err:#}").contains("refusing to serve unauthenticated tracker"));
    }

    #[test]
    fn tracker_serve_auth_guard_allows_token_loopback_or_explicit_opt_in() {
        assert!(validate_tracker_serve_auth("0.0.0.0:8850", Some("secret"), false).is_ok());
        assert!(validate_tracker_serve_auth("127.0.0.1:8850", None, false).is_ok());
        assert!(validate_tracker_serve_auth("0.0.0.0:8850", None, true).is_ok());
    }

    #[test]
    fn doctor_parses() {
        let app = App::parse_from(["rr", "doctor"]);
        match app.cmd {
            Command::Doctor { json, auto_fix } => {
                assert!(!json);
                assert!(!auto_fix);
            }
            _ => panic!("expected doctor"),
        }
    }

    #[test]
    fn doctor_parses_json_flag() {
        let app = App::parse_from(["rr", "doctor", "--json"]);
        match app.cmd {
            Command::Doctor { json, auto_fix } => {
                assert!(json);
                assert!(!auto_fix);
            }
            _ => panic!("expected doctor"),
        }
    }

    #[test]
    fn doctor_parses_auto_fix_flag() {
        let app = App::parse_from(["rr", "doctor", "--auto-fix"]);
        match app.cmd {
            Command::Doctor { json, auto_fix } => {
                assert!(!json);
                assert!(auto_fix);
            }
            _ => panic!("expected doctor"),
        }
    }

    #[test]
    fn config_load_error_policy_keeps_doctor_running() {
        let app = App::parse_from(["rr", "doctor"]);

        assert!(can_continue_after_config_load_error(&app.cmd));
    }

    #[test]
    fn version_command_parses_and_ignores_config_load_errors() {
        let app = App::parse_from(["rr", "version", "--json"]);

        match app.cmd {
            Command::Version { json } => assert!(json),
            _ => panic!("expected version"),
        }
        assert!(can_continue_after_config_load_error(&app.cmd));
    }

    #[test]
    fn update_command_parses_and_ignores_config_load_errors() {
        let app = App::parse_from([
            "rr",
            "update",
            "--version",
            "v1.0.2",
            "--repo",
            "zrma/rustory",
            "--asset-base-url",
            "https://example.test/releases/v1.0.2",
            "--install-path",
            "/tmp/rr",
            "--no-restart-daemon",
            "--dry-run",
        ]);

        match &app.cmd {
            Command::Update {
                version,
                repo,
                asset_base_url,
                install_path,
                dry_run,
                no_restart_daemon,
                ..
            } => {
                assert_eq!(version, "v1.0.2");
                assert_eq!(repo, "zrma/rustory");
                assert_eq!(
                    asset_base_url.as_deref(),
                    Some("https://example.test/releases/v1.0.2")
                );
                assert_eq!(install_path.as_deref(), Some("/tmp/rr"));
                assert!(*dry_run);
                assert!(*no_restart_daemon);
            }
            _ => panic!("expected update"),
        }
        assert!(can_continue_after_config_load_error(&app.cmd));
    }

    #[test]
    fn clap_version_includes_build_revision() {
        use clap::CommandFactory;

        let cmd = App::command();
        let version = cmd.get_version().expect("version");

        assert!(version.contains(crate::build_info::VERSION));
        assert!(version.contains(crate::build_info::BUILD_REVISION));
    }

    #[test]
    fn config_load_error_policy_allows_only_force_init() {
        let force_app = App::parse_from(["rr", "init", "--force"]);
        let non_force_app = App::parse_from(["rr", "init"]);
        let search_app = App::parse_from(["rr", "search"]);

        assert!(can_continue_after_config_load_error(&force_app.cmd));
        assert!(!can_continue_after_config_load_error(&non_force_app.cmd));
        assert!(!can_continue_after_config_load_error(&search_app.cmd));
    }

    #[test]
    fn doctor_report_builds_json_shape() {
        let report = build_doctor_report(&config::FileConfig::default(), ":memory:", None).unwrap();
        assert_eq!(report.db_path, ":memory:");
        assert_eq!(report.db.path, ":memory:");
        assert!(report.config_error.is_none());
        assert!(!report.db.exists);
        assert!(report.config_warning.is_none());
        assert!(!report.tracker_token.configured);
        assert!(report.trackers.is_empty());
        assert!(report.p2p_request_retry.error.is_none());

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"build\""));
        assert!(json.contains("\"db\""));
        assert!(json.contains("\"config_error\""));
        assert!(json.contains("\"config_warning\""));
        assert!(json.contains("\"hook\""));
        assert!(json.contains("\"async_upload\""));
        assert!(json.contains("\"auto_prune\""));
        assert!(json.contains("\"relay_addr\""));
        assert!(json.contains("\"tracker_token\""));
    }

    #[test]
    fn doctor_report_includes_tracker_token_status_without_value() {
        let cfg = config::FileConfig {
            tracker_token: Some("short-secret".to_string()),
            ..Default::default()
        };

        let report = build_doctor_report(&cfg, ":memory:", None).unwrap();
        assert!(report.tracker_token.configured);
        assert_eq!(report.tracker_token.length, Some("short-secret".len()));
        assert!(
            report
                .tracker_token
                .warning
                .as_deref()
                .unwrap()
                .contains("short")
        );
        assert!(report.tracker_token.error.is_none());

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"length\""));
        assert!(!json.contains("short-secret"));
    }

    #[test]
    fn doctor_report_rejects_quote_wrapped_tracker_token_without_value() {
        let cfg = config::FileConfig {
            tracker_token: Some("'secret-token-value'".to_string()),
            ..Default::default()
        };

        let report = build_doctor_report(&cfg, ":memory:", None).unwrap();
        assert!(report.tracker_token.configured);
        assert_eq!(
            report.tracker_token.length,
            Some("'secret-token-value'".len())
        );
        assert!(
            report
                .tracker_token
                .warning
                .as_deref()
                .unwrap()
                .contains("literal quote")
        );
        assert!(
            report
                .tracker_token
                .error
                .as_deref()
                .unwrap()
                .contains("literal quote")
        );

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"length\""));
        assert!(!json.contains("secret-token-value"));
    }

    #[test]
    fn tracker_token_report_warns_when_trackers_have_no_token() {
        let report = build_tracker_token_report(None, true);
        assert!(!report.configured);
        assert!(
            report
                .warning
                .as_deref()
                .unwrap()
                .contains("unauthenticated")
        );
    }

    #[test]
    fn doctor_report_records_config_error() {
        let report = build_doctor_report(
            &config::FileConfig::default(),
            ":memory:",
            Some("parse config toml: invalid TOML"),
        )
        .unwrap();

        assert_eq!(
            report.config_error.as_deref(),
            Some("parse config toml: invalid TOML")
        );

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"config_error\""));
        assert!(json.contains("parse config toml"));
    }

    #[test]
    fn doctor_report_includes_existing_db_counts() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.db");
        let store = storage::LocalStore::open(db_path.to_str().unwrap()).unwrap();
        store
            .insert_entries(&[crate::core::Entry {
                entry_id: "id-1".to_string(),
                device_id: "dev1".to_string(),
                user_id: "user1".to_string(),
                ts: time::OffsetDateTime::from_unix_timestamp(1).unwrap(),
                cmd: "echo test".to_string(),
                cwd: "/tmp".to_string(),
                exit_code: 0,
                duration_ms: 10,
                shell: "zsh".to_string(),
                hostname: "host".to_string(),
                version: crate::build_info::VERSION.to_string(),
            }])
            .unwrap();
        store
            .upsert_peer_book(&storage::PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1111/p2p/peer-a".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some("dev2".to_string()),
                last_seen_unix: 99,
            })
            .unwrap();
        store.set_last_cursor("peer-a", 1).unwrap();
        drop(store);

        let report = build_doctor_report(
            &config::FileConfig::default(),
            db_path.to_str().unwrap(),
            None,
        )
        .unwrap();
        assert!(report.db.exists);
        assert_eq!(report.db.entry_count, Some(1));
        assert_eq!(report.db.latest_ingest_seq, Some(1));
        assert_eq!(report.db.peer_book_count, Some(1));
        assert_eq!(report.db.sync_peer_count, Some(1));
        assert!(report.db.error.is_none());
    }

    #[test]
    fn hook_status_reports_missing_marker_with_default_limit() {
        let report =
            build_hook_status_report_from_env(Some("zsh".to_string()), None, None, None, None);

        assert!(!report.installed);
        assert!(!report.disabled);
        assert_eq!(report.shell.as_deref(), Some("zsh"));
        assert_eq!(report.search_limit, Some(DEFAULT_HOOK_SEARCH_LIMIT));
        assert!(report.error.is_none());
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("hook marker missing"))
        );
    }

    #[test]
    fn hook_status_reports_disable_and_invalid_limit() {
        let report = build_hook_status_report_from_env(
            Some("fish".to_string()),
            Some("1".to_string()),
            Some("1".to_string()),
            Some("many".to_string()),
            Some(42),
        );

        assert!(report.installed);
        assert!(report.disabled);
        assert_eq!(report.shell.as_deref(), Some("fish"));
        assert!(report.search_limit.is_none());
        assert!(
            report
                .error
                .as_deref()
                .unwrap()
                .contains("invalid RUSTORY_SEARCH_LIMIT")
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("RUSTORY_HOOK_DISABLE"))
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("unsupported shell"))
        );
    }

    #[test]
    fn hook_status_accepts_disable_false_values() {
        for value in ["0", "false", "no", "off"] {
            let report = build_hook_status_report_from_env(
                Some("zsh".to_string()),
                Some("1".to_string()),
                Some(value.to_string()),
                None,
                Some(42),
            );

            assert!(report.installed);
            assert!(!report.disabled, "value={value}");
            assert_eq!(report.search_limit, Some(42));
            assert!(report.error.is_none());
            assert!(
                !report
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("RUSTORY_HOOK_DISABLE is set")),
                "value={value}"
            );
        }
    }

    #[test]
    fn hook_status_disables_on_invalid_disable_value_for_safety() {
        let report = build_hook_status_report_from_env(
            Some("zsh".to_string()),
            Some("1".to_string()),
            Some("maybe".to_string()),
            None,
            Some(42),
        );

        assert!(report.disabled);
        assert!(report.error.is_none());
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("invalid RUSTORY_HOOK_DISABLE"))
        );
    }

    #[test]
    fn hook_status_uses_config_search_limit_when_env_is_absent() {
        let report = build_hook_status_report_from_env(
            Some("zsh".to_string()),
            Some("1".to_string()),
            None,
            None,
            Some(42),
        );

        assert!(report.installed);
        assert_eq!(report.search_limit, Some(42));
        assert!(report.error.is_none());
    }

    #[test]
    fn hook_status_env_search_limit_overrides_config() {
        let report = build_hook_status_report_from_env(
            Some("zsh".to_string()),
            Some("1".to_string()),
            None,
            Some("7".to_string()),
            Some(42),
        );

        assert!(report.installed);
        assert_eq!(report.search_limit, Some(7));
        assert!(report.error.is_none());
    }

    #[test]
    fn doctor_report_keeps_running_when_swarm_key_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let invalid_swarm_key = dir.path().join("swarm-invalid.key");
        std::fs::write(&invalid_swarm_key, "invalid-swarm-key").unwrap();

        let cfg = config::FileConfig {
            swarm_key_path: Some(invalid_swarm_key.display().to_string()),
            ..Default::default()
        };

        let report = build_doctor_report(&cfg, ":memory:", None).unwrap();
        assert!(report.swarm_key.exists);
        assert!(report.swarm_key.value.is_none());
        assert!(report.swarm_key.load_error.is_some());
        assert!(
            report
                .swarm_key
                .load_error
                .as_deref()
                .unwrap()
                .contains("parse swarm key")
        );
    }

    #[test]
    fn doctor_text_output_keeps_running_when_swarm_key_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let invalid_swarm_key = dir.path().join("swarm-invalid.key");
        std::fs::write(&invalid_swarm_key, "invalid-swarm-key").unwrap();

        let cfg = config::FileConfig {
            swarm_key_path: Some(invalid_swarm_key.display().to_string()),
            ..Default::default()
        };

        assert!(run_doctor(&cfg, ":memory:", false, false, None).is_ok());
    }

    #[test]
    fn doctor_text_output_keeps_running_when_config_is_invalid() {
        assert!(
            run_doctor(
                &config::FileConfig::default(),
                ":memory:",
                false,
                false,
                Some("parse config toml: invalid TOML")
            )
            .is_ok()
        );
    }

    #[test]
    fn doctor_text_output_keeps_running_when_tracker_token_is_invalid() {
        let cfg = config::FileConfig {
            trackers: Some(vec!["http://127.0.0.1:1".to_string()]),
            tracker_token: Some("abc\nxyz".to_string()),
            ..Default::default()
        };

        assert!(run_doctor(&cfg, ":memory:", false, false, None).is_ok());
    }

    #[test]
    fn doctor_auto_fix_sets_default_record_ignore_regex() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("config.toml");
        std::fs::write(&cfg_path, "db_path = \"~/.rustory/history.db\"\n").unwrap();

        let cfg = config::FileConfig::default();
        let mut report = DoctorAutoFixReport::default();
        fix_default_record_ignore_regex(&cfg, &cfg_path, None, &mut report).unwrap();

        let content = std::fs::read_to_string(&cfg_path).unwrap();
        let loaded: config::FileConfig = toml::from_str(&content).unwrap();
        assert_eq!(
            loaded.record_ignore_regex.as_deref(),
            Some(DEFAULT_RECORD_IGNORE_REGEX)
        );
        assert!(report.actions.iter().any(|action| {
            action.target == "record_ignore_regex" && action.status == DoctorAutoFixStatus::Fixed
        }));
    }

    #[cfg(unix)]
    #[test]
    fn doctor_auto_fix_sets_private_path_modes() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("history.db");
        std::fs::write(&db_path, "").unwrap();
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let mut report = DoctorAutoFixReport::default();
        fix_path_mode(&db_path, 0o600, "db file", &mut report).unwrap();

        assert_eq!(file_mode_777(&db_path), Some(0o600));
        assert!(report.actions.iter().any(|action| {
            action.target == "db file" && action.status == DoctorAutoFixStatus::Fixed
        }));
    }

    #[test]
    fn sync_status_parses_peer_filter() {
        let app = App::parse_from(["rr", "sync-status", "--peer", "peer-a"]);
        match app.cmd {
            Command::SyncStatus {
                peer,
                json,
                with_tracker,
                watch,
                interval_sec,
            } => {
                assert_eq!(peer.as_deref(), Some("peer-a"));
                assert!(!json);
                assert!(!with_tracker);
                assert!(!watch);
                assert_eq!(interval_sec, 2);
            }
            _ => panic!("expected sync-status"),
        }
    }

    #[test]
    fn sync_status_parses_json_flag() {
        let app = App::parse_from(["rr", "sync-status", "--json"]);
        match app.cmd {
            Command::SyncStatus {
                peer,
                json,
                with_tracker,
                watch,
                interval_sec,
            } => {
                assert!(peer.is_none());
                assert!(json);
                assert!(!with_tracker);
                assert!(!watch);
                assert_eq!(interval_sec, 2);
            }
            _ => panic!("expected sync-status"),
        }
    }

    #[test]
    fn sync_status_parses_with_tracker_flag() {
        let app = App::parse_from(["rr", "sync-status", "--with-tracker"]);
        match app.cmd {
            Command::SyncStatus {
                peer,
                json,
                with_tracker,
                watch,
                interval_sec,
            } => {
                assert!(peer.is_none());
                assert!(!json);
                assert!(with_tracker);
                assert!(!watch);
                assert_eq!(interval_sec, 2);
            }
            _ => panic!("expected sync-status"),
        }
    }

    #[test]
    fn sync_status_parses_watch_flags() {
        let app = App::parse_from(["rr", "sync-status", "--watch", "--interval-sec", "5"]);
        match app.cmd {
            Command::SyncStatus {
                peer,
                json,
                with_tracker,
                watch,
                interval_sec,
            } => {
                assert!(peer.is_none());
                assert!(!json);
                assert!(!with_tracker);
                assert!(watch);
                assert_eq!(interval_sec, 5);
            }
            _ => panic!("expected sync-status"),
        }
    }

    #[test]
    fn daemon_parses_daily_driver_flags() {
        let app = App::parse_from([
            "rr",
            "daemon",
            "--trackers",
            "http://127.0.0.1:8850,http://127.0.0.1:8851",
            "--relay",
            "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWQUL3M8e18xRRXQ1ZHtVYoYoWq7HvM4hFQXQeZDA5B3eB",
            "--tracker-token",
            "secret-token",
            "--interval-sec",
            "30",
            "--start-jitter-sec",
            "5",
            "--sync-start-delay-sec",
            "1",
            "--max-peers-per-tick",
            "3",
            "--preflight",
            "--req-attempts",
            "4",
        ]);
        match app.cmd {
            Command::Daemon {
                trackers,
                relay,
                tracker_token,
                limit,
                pull_only,
                interval_sec,
                start_jitter_sec,
                sync_start_delay_sec,
                max_peers_per_tick,
                preflight,
                req_attempts,
                ..
            } => {
                assert_eq!(trackers.len(), 2);
                assert!(relay.as_deref().unwrap().contains("/p2p/"));
                assert_eq!(tracker_token.as_deref(), Some("secret-token"));
                assert_eq!(limit, 1000);
                assert!(!pull_only);
                assert_eq!(interval_sec, 30);
                assert_eq!(start_jitter_sec, Some(5));
                assert_eq!(sync_start_delay_sec, 1);
                assert_eq!(max_peers_per_tick, 3);
                assert!(preflight);
                assert_eq!(req_attempts, Some(4));
            }
            _ => panic!("expected daemon"),
        }
    }

    #[test]
    fn daemon_preflight_status_validation_requires_all_trackers_reachable() {
        let ok = vec![SyncStatusTrackerReport {
            base_url: "http://tracker-a".to_string(),
            reachable: true,
            latency_ms: Some(1),
            error: None,
        }];
        assert!(validate_daemon_preflight_statuses(&ok).is_ok());

        let mixed = vec![
            SyncStatusTrackerReport {
                base_url: "http://tracker-a".to_string(),
                reachable: true,
                latency_ms: Some(1),
                error: None,
            },
            SyncStatusTrackerReport {
                base_url: "http://tracker-b".to_string(),
                reachable: false,
                latency_ms: None,
                error: Some("status 401".to_string()),
            },
        ];
        let err = validate_daemon_preflight_statuses(&mixed).unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("daemon preflight failed"));
        assert!(text.contains("http://tracker-b"));
        assert!(text.contains("status 401"));
    }

    #[test]
    fn daemon_child_specs_push_by_default_and_do_not_leak_token() {
        let args = DaemonArgs {
            listen: "/ip4/0.0.0.0/tcp/0".to_string(),
            identity_key: Some("/tmp/identity.key".to_string()),
            swarm_key: Some("/tmp/swarm.key".to_string()),
            relay: Some("/ip4/127.0.0.1/tcp/4001/p2p/relay".to_string()),
            trackers: vec!["http://127.0.0.1:8850".to_string()],
            tracker_token: Some("secret-token".to_string()),
            limit: 500,
            pull_only: false,
            interval_sec: 0,
            start_jitter_sec: Some(3),
            sync_start_delay_sec: 2,
            max_peers_per_tick: 1,
            preflight: false,
            req_attempts: Some(4),
            req_timeout_base_sec: None,
            req_timeout_cap_sec: None,
            req_backoff_base_ms: None,
        };

        let specs = build_daemon_child_specs("/tmp/rustory.db", &args);
        assert_eq!(specs.tracker_token_env.as_deref(), Some("secret-token"));
        assert!(specs.sync_args.contains(&"--push".to_string()));
        assert!(
            specs
                .sync_args
                .windows(2)
                .any(|pair| pair == ["--max-peers-per-tick", "1"])
        );
        assert!(!specs.serve_args.iter().any(|arg| arg == "secret-token"));
        assert!(!specs.sync_args.iter().any(|arg| arg == "secret-token"));
    }

    #[test]
    fn daemon_child_specs_pull_only_omits_push() {
        let args = DaemonArgs {
            listen: "/ip4/0.0.0.0/tcp/0".to_string(),
            identity_key: None,
            swarm_key: None,
            relay: None,
            trackers: Vec::new(),
            tracker_token: None,
            limit: 1000,
            pull_only: true,
            interval_sec: 60,
            start_jitter_sec: None,
            sync_start_delay_sec: 2,
            max_peers_per_tick: 1,
            preflight: false,
            req_attempts: None,
            req_timeout_base_sec: None,
            req_timeout_cap_sec: None,
            req_backoff_base_ms: None,
        };

        let specs = build_daemon_child_specs("/tmp/rustory.db", &args);
        assert!(!specs.sync_args.contains(&"--push".to_string()));
    }

    #[test]
    fn prune_parses_flags() {
        let app = App::parse_from([
            "rr",
            "prune",
            "--older-than-days",
            "30",
            "--keep-recent",
            "200",
            "--dry-run",
        ]);
        match app.cmd {
            Command::Prune {
                older_than_days,
                keep_recent,
                dry_run,
            } => {
                assert_eq!(older_than_days, 30);
                assert_eq!(keep_recent, Some(200));
                assert!(dry_run);
            }
            _ => panic!("expected prune"),
        }
    }

    #[test]
    fn delete_parses_selectors_and_safety_flags() {
        let app = App::parse_from([
            "rr",
            "delete",
            "--entry-id",
            "id-1,id-2",
            "--cmd-regex",
            "(?i)token",
            "--dry-run",
            "--yes",
            "--vacuum",
        ]);
        match app.cmd {
            Command::Delete {
                entry_id,
                cmd_regex,
                dry_run,
                yes,
                vacuum,
            } => {
                assert_eq!(entry_id, vec!["id-1", "id-2"]);
                assert_eq!(cmd_regex.as_deref(), Some("(?i)token"));
                assert!(dry_run);
                assert!(yes);
                assert!(vacuum);
            }
            _ => panic!("expected delete"),
        }
    }

    #[test]
    fn sync_status_report_includes_pending_push_and_filter() {
        use time::OffsetDateTime;

        fn entry(entry_id: &str, ts: i64, device_id: &str) -> crate::core::Entry {
            crate::core::Entry {
                entry_id: entry_id.to_string(),
                device_id: device_id.to_string(),
                user_id: "user1".to_string(),
                ts: OffsetDateTime::from_unix_timestamp(ts).unwrap(),
                cmd: "echo test".to_string(),
                cwd: "/tmp".to_string(),
                exit_code: 0,
                duration_ms: 10,
                shell: "zsh".to_string(),
                hostname: "host".to_string(),
                version: crate::build_info::VERSION.to_string(),
            }
        }

        let store = storage::LocalStore::open(":memory:").unwrap();
        store
            .insert_entries(&[
                entry("id-1", 1, "dev-local"),
                entry("id-2", 2, "dev-remote"),
                entry("id-3", 3, "dev-local"),
            ])
            .unwrap();
        store.set_last_cursor("peer-a", 2).unwrap();
        store.set_last_pushed_seq("peer-a", 1).unwrap();
        store.set_last_cursor("peer-b", 3).unwrap();
        store.set_last_pushed_seq("peer-b", 3).unwrap();
        store.set_last_cursor("peer-self", 1).unwrap();
        store.set_last_cursor("peer-local-id", 1).unwrap();
        store.set_last_cursor("peer-self-spaced", 1).unwrap();
        store
            .upsert_peer_book(&storage::PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1111/p2p/peer-a".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some("dev-remote".to_string()),
                last_seen_unix: 99,
            })
            .unwrap();
        store
            .upsert_peer_book(&storage::PeerBookPeer {
                peer_id: "peer-self".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/2222/p2p/peer-self".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some("dev-local".to_string()),
                last_seen_unix: 100,
            })
            .unwrap();
        store
            .upsert_peer_book(&storage::PeerBookPeer {
                peer_id: "peer-self-spaced".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/3333/p2p/peer-self-spaced".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some(" dev-local ".to_string()),
                last_seen_unix: 101,
            })
            .unwrap();

        let report =
            build_sync_status_report(&store, "dev-local", Some("peer-local-id"), None, None)
                .unwrap();
        assert_eq!(report.local_head, 3);
        assert_eq!(report.local_device_id, "dev-local");
        assert_eq!(report.peers.len(), 2);
        assert!(report.tracker_status.is_none());
        assert!(!report.peers.iter().any(|peer| peer.peer_id == "peer-self"));
        assert!(
            !report
                .peers
                .iter()
                .any(|peer| peer.peer_id == "peer-local-id")
        );
        assert!(
            !report
                .peers
                .iter()
                .any(|peer| peer.peer_id == "peer-self-spaced")
        );

        let peer_a = report
            .peers
            .iter()
            .find(|peer| peer.peer_id == "peer-a")
            .unwrap();
        assert_eq!(peer_a.pull_cursor, 2);
        assert_eq!(peer_a.push_cursor, 1);
        assert_eq!(peer_a.peer_device_id.as_deref(), Some("dev-remote"));
        assert_eq!(peer_a.outbound_push_pending, 1);
        assert_eq!(peer_a.pending_push, 1);
        assert_eq!(peer_a.last_seen_unix, Some(99));
        assert!(peer_a.last_seen_age_sec.is_some());

        let peer_b = report
            .peers
            .iter()
            .find(|peer| peer.peer_id == "peer-b")
            .unwrap();
        assert_eq!(peer_b.outbound_push_pending, 0);
        assert_eq!(peer_b.pending_push, 0);
        assert_eq!(peer_b.last_seen_unix, None);
        assert_eq!(peer_b.last_seen_age_sec, None);

        let filtered = build_sync_status_report(
            &store,
            "dev-local",
            Some("peer-local-id"),
            Some("peer-a"),
            None,
        )
        .unwrap();
        assert_eq!(filtered.peers.len(), 1);
        assert_eq!(filtered.peers[0].peer_id, "peer-a");

        let json = serde_json::to_string(&filtered).unwrap();
        assert!(json.contains("\"local_head\""));
        assert!(json.contains("\"local_device_id\""));
        assert!(json.contains("\"peer_device_id\""));
        assert!(json.contains("\"outbound_push_pending\""));
        assert!(json.contains("\"pending_push\""));
        assert!(json.contains("\"last_seen_unix\""));
        assert!(json.contains("\"last_seen_age_sec\""));
    }

    #[test]
    fn sync_status_watch_progress_helpers_are_stable() {
        assert_eq!(outbound_push_progress_percent(0, 0), 100);
        assert_eq!(outbound_push_progress_percent(50, 100), 50);
        assert_eq!(outbound_push_progress_percent(150, 100), 0);

        assert_eq!(progress_bar(0, 4), "░░░░");
        assert_eq!(progress_bar(50, 4), "██░░");
        assert_eq!(progress_bar(100, 4), "████");

        assert_eq!(format_rate(42.4), "42");
        assert_eq!(format_rate(1200.0), "1.2k");
    }

    #[test]
    fn sync_status_watch_frame_stays_bounded_for_long_values() {
        let mut state = SyncStatusWatchState::default();
        let report = SyncStatusReport {
            local_head: 2_129_846,
            local_device_id: "user-arm64-with-an-extra-long-local-device-id".to_string(),
            peers: vec![
                SyncStatusPeerReport {
                    peer_id: "12D3KooWE3u4VEsbCGR7w53rbBYi1mZ3kADAgAhDYTj8ACiPBC1M".to_string(),
                    peer_device_id: Some(
                        "sample-node-x86_64-with-a-very-long-device-name".to_string(),
                    ),
                    pull_cursor: 1_526_049,
                    push_cursor: 1_968_089,
                    outbound_push_pending: 2_311,
                    pending_push: 2_311,
                    last_seen_unix: Some(1),
                    last_seen_age_sec: Some(7),
                },
                SyncStatusPeerReport {
                    peer_id: "12D3KooWKvNkdisp13vqjrzZtPkDUz1aB2uVYpWBQCDVT3ihPcJU".to_string(),
                    peer_device_id: Some("node3".to_string()),
                    pull_cursor: 1_818_365,
                    push_cursor: 2_122_722,
                    outbound_push_pending: 0,
                    pending_push: 0,
                    last_seen_unix: Some(1),
                    last_seen_age_sec: Some(123_456),
                },
            ],
            tracker_status: Some(vec![SyncStatusTrackerReport {
                base_url: "https://tracker.example.com/with/a/long/path".to_string(),
                reachable: false,
                latency_ms: None,
                error: Some("timeout: connect with a long transport error message".to_string()),
            }]),
        };

        let frame = render_sync_status_watch_frame(&mut state, &report, Instant::now(), 160);

        assert!(frame.contains("rustory mesh watch"));
        assert!(frame.contains("Mesh Map"));
        assert!(frame.contains("Traffic"));
        assert!(frame.contains("Links"));
        assert!(frame.contains("pull_cur"));
        assert!(frame.contains("pull/s"));
        assert!(frame.contains("push_cur"));
        assert!(frame.contains("pending"));
        assert!(frame.contains("drain/s"));
        assert!(frame.contains("2.3k"));
        assert!(frame.contains("2.0M"));
        assert!(frame.contains("◇"));
        for line in frame.lines() {
            let width = unicode_width::UnicodeWidthStr::width(line);
            assert!(width <= 160, "line width {width}: {line}");
        }
    }

    #[test]
    fn sync_status_watch_status_lines_align_right_columns() {
        let backlog = traffic_backlog_line(5, 16, 52);
        let hot = traffic_hot_line("node0 12D3KooWQJ8wUaWhMxSGwGD65PsQFoYaR", 1_968_089, 13, 52);
        let progress = link_push_progress_line(66, 24);

        assert_eq!(unicode_width::UnicodeWidthStr::width(backlog.as_str()), 52);
        assert_eq!(unicode_width::UnicodeWidthStr::width(hot.as_str()), 52);
        assert_eq!(unicode_width::UnicodeWidthStr::width(progress.as_str()), 24);
        assert!(backlog.ends_with("   5%      16 left"));
        assert!(hot.ends_with("cur    2.0M       13 left"));
        assert!(progress.ends_with("  66%"));
    }

    #[test]
    fn compute_last_seen_age_sec_handles_past_and_future() {
        assert_eq!(compute_last_seen_age_sec(100, Some(90)), Some(10));
        assert_eq!(compute_last_seen_age_sec(100, Some(100)), Some(0));
        assert_eq!(compute_last_seen_age_sec(100, Some(110)), Some(0));
        assert_eq!(compute_last_seen_age_sec(100, None), None);
    }

    #[test]
    fn compute_prune_cutoff_unix_validates_and_calculates() {
        assert_eq!(compute_prune_cutoff_unix(900_000, 2).unwrap(), 727_200);
        assert!(compute_prune_cutoff_unix(900_000, 0).is_err());
        assert!(compute_prune_cutoff_unix(900_000, u64::MAX).is_err());
    }

    #[test]
    fn parse_env_bool_accepts_common_values() {
        assert!(parse_env_bool("1", "X").unwrap());
        assert!(parse_env_bool("true", "X").unwrap());
        assert!(parse_env_bool("yes", "X").unwrap());
        assert!(parse_env_bool("on", "X").unwrap());
        assert!(!parse_env_bool("0", "X").unwrap());
        assert!(!parse_env_bool("false", "X").unwrap());
        assert!(!parse_env_bool("no", "X").unwrap());
        assert!(!parse_env_bool("off", "X").unwrap());
    }

    #[test]
    fn parse_env_bool_rejects_invalid_values() {
        assert!(parse_env_bool("maybe", "X").is_err());
        assert!(parse_env_bool("", "X").is_err());
    }

    #[test]
    fn runtime_settings_use_config_fallback_when_env_is_absent() {
        assert!(resolve_bool_setting("RUSTORY_ASYNC_UPLOAD", None, Some(true), false).unwrap());
        assert_eq!(
            resolve_u64_setting(
                "RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC",
                "async_upload_interval_sec",
                None,
                Some(30),
                DEFAULT_ASYNC_UPLOAD_INTERVAL_SEC,
                1,
            )
            .unwrap(),
            30
        );
        assert_eq!(
            resolve_usize_setting(
                "RUSTORY_ASYNC_UPLOAD_LIMIT",
                "async_upload_limit",
                None,
                Some(500),
                DEFAULT_ASYNC_UPLOAD_LIMIT,
                1,
            )
            .unwrap(),
            500
        );
        assert_eq!(
            resolve_string_setting(
                None,
                Some("~/custom-runtime.last".to_string()),
                DEFAULT_ASYNC_UPLOAD_MARKER_PATH,
            ),
            "~/custom-runtime.last"
        );
    }

    #[test]
    fn runtime_settings_env_override_config_values() {
        assert!(
            !resolve_bool_setting(
                "RUSTORY_ASYNC_UPLOAD",
                Some("0".to_string()),
                Some(true),
                false,
            )
            .unwrap()
        );
        assert_eq!(
            resolve_u64_setting(
                "RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC",
                "async_upload_interval_sec",
                Some("7".to_string()),
                Some(30),
                DEFAULT_ASYNC_UPLOAD_INTERVAL_SEC,
                1,
            )
            .unwrap(),
            7
        );
        assert_eq!(
            resolve_string_setting(
                Some("~/env-runtime.last".to_string()),
                Some("~/config-runtime.last".to_string()),
                DEFAULT_ASYNC_UPLOAD_MARKER_PATH,
            ),
            "~/env-runtime.last"
        );
    }

    #[test]
    fn search_limit_uses_config_default_when_env_is_absent() {
        assert_eq!(
            resolve_search_limit_from_values(None, None, Some(42)).unwrap(),
            42
        );
    }

    #[test]
    fn search_limit_env_overrides_config_default() {
        assert_eq!(
            resolve_search_limit_from_values(None, Some("7".to_string()), Some(42)).unwrap(),
            7
        );
    }

    #[test]
    fn search_limit_cli_overrides_env_and_config() {
        assert_eq!(
            resolve_search_limit_from_values(Some(3), Some("7".to_string()), Some(42)).unwrap(),
            3
        );
    }

    #[test]
    fn search_limit_rejects_invalid_env_value() {
        assert!(
            resolve_search_limit_from_values(None, Some("many".to_string()), Some(42))
                .unwrap_err()
                .to_string()
                .contains("invalid RUSTORY_SEARCH_LIMIT")
        );
    }

    #[test]
    fn runtime_settings_reject_zero_config_values() {
        assert!(
            resolve_u64_setting(
                "RUSTORY_AUTO_PRUNE_DAYS",
                "auto_prune_days",
                None,
                Some(0),
                DEFAULT_AUTO_PRUNE_DAYS,
                1,
            )
            .unwrap_err()
            .to_string()
            .contains("auto_prune_days must be >= 1")
        );
        assert!(
            resolve_usize_setting(
                "RUSTORY_ASYNC_UPLOAD_LIMIT",
                "async_upload_limit",
                None,
                Some(0),
                DEFAULT_ASYNC_UPLOAD_LIMIT,
                1,
            )
            .unwrap_err()
            .to_string()
            .contains("async_upload_limit must be >= 1")
        );
    }

    #[test]
    fn should_trigger_interval_respects_interval() {
        assert!(should_trigger_interval(100, None, 15));
        assert!(should_trigger_interval(100, Some(80), 15));
        assert!(!should_trigger_interval(100, Some(90), 15));
        assert!(!should_trigger_interval(100, Some(110), 15));
    }

    #[test]
    fn compute_next_due_in_sec_handles_missing_and_elapsed_markers() {
        assert_eq!(compute_next_due_in_sec(100, None, 15), 0);
        assert_eq!(compute_next_due_in_sec(100, Some(80), 15), 0);
        assert_eq!(compute_next_due_in_sec(100, Some(90), 15), 5);
        assert_eq!(compute_next_due_in_sec(100, Some(110), 15), 15);
    }

    #[test]
    fn summarize_async_upload_runtime_reports_marker_and_next_due() {
        let report = summarize_async_upload_runtime(
            AsyncUploadRuntimeSettings {
                enabled: true,
                interval_sec: 15,
                limit: 200,
                marker_path: std::path::PathBuf::from("/tmp/async-upload.last"),
                last_trigger_unix: Some(95),
            },
            100,
        );

        assert!(report.enabled);
        assert_eq!(report.interval_sec, 15);
        assert_eq!(report.limit, 200);
        assert_eq!(report.last_trigger_unix, Some(95));
        assert_eq!(report.next_due_in_sec, 10);
    }

    #[test]
    fn summarize_auto_prune_runtime_reports_marker_and_next_due() {
        let report = summarize_auto_prune_runtime(
            AutoPruneRuntimeSettings {
                enabled: true,
                older_than_days: 180,
                interval_sec: 86_400,
                keep_recent: 5000,
                marker_path: std::path::PathBuf::from("/tmp/auto-prune.last"),
                last_trigger_unix: Some(1_000_000),
            },
            1_000_300,
        );

        assert!(report.enabled);
        assert_eq!(report.older_than_days, 180);
        assert_eq!(report.interval_sec, 86_400);
        assert_eq!(report.keep_recent, 5000);
        assert_eq!(report.last_trigger_unix, Some(1_000_000));
        assert_eq!(report.next_due_in_sec, 86_100);
    }

    #[test]
    fn rate_limit_marker_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("async-upload.last");

        assert_eq!(read_rate_limit_marker(&marker).unwrap(), None);

        write_rate_limit_marker(&marker, 1234).unwrap();
        assert_eq!(read_rate_limit_marker(&marker).unwrap(), Some(1234));
    }

    #[test]
    fn tracker_status_report_marks_unreachable_on_ping_error() {
        let reports = build_tracker_status_report(&["http://127.0.0.1:0".to_string()], None);
        assert_eq!(reports.len(), 1);
        assert!(!reports[0].reachable);
        assert!(reports[0].latency_ms.is_none());
        assert!(reports[0].error.is_some());
    }

    #[test]
    fn tracker_status_report_includes_latency_on_success() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let server = tiny_http::Server::http(addr).unwrap();
        let handle = std::thread::spawn(move || {
            let req = server.recv().unwrap();
            let path = req.url().split('?').next().unwrap_or(req.url());
            let status = if path == "/api/v1/ping" { 200 } else { 404 };
            let response = tiny_http::Response::empty(tiny_http::StatusCode(status));
            req.respond(response).unwrap();
        });

        let reports = build_tracker_status_report(&[format!("http://{addr}")], None);
        assert_eq!(reports.len(), 1);
        assert!(reports[0].reachable);
        assert!(reports[0].latency_ms.is_some());
        assert!(reports[0].error.is_none());
        assert!(
            serde_json::to_string(&reports[0])
                .unwrap()
                .contains("\"latency_ms\"")
        );

        handle.join().unwrap();
    }

    #[test]
    fn init_parses_flags() {
        let app = App::parse_from([
            "rr",
            "init",
            "--force",
            "--user-id",
            "u1",
            "--device-id",
            "d1",
            "--trackers",
            "http://127.0.0.1:8850,http://127.0.0.1:8851",
            "--relay",
            "/ip4/127.0.0.1/tcp/4001",
            "--tracker-token",
            "t1",
        ]);

        match app.cmd {
            Command::Init {
                force,
                user_id,
                device_id,
                trackers,
                relay,
                tracker_token,
            } => {
                assert!(force);
                assert_eq!(user_id.as_deref(), Some("u1"));
                assert_eq!(device_id.as_deref(), Some("d1"));
                assert_eq!(trackers.len(), 2);
                assert_eq!(relay.as_deref(), Some("/ip4/127.0.0.1/tcp/4001"));
                assert_eq!(tracker_token.as_deref(), Some("t1"));
            }
            _ => panic!("expected init"),
        }
    }

    #[test]
    fn init_parses_tracker_and_token_aliases() {
        let app = App::parse_from([
            "rr",
            "init",
            "--tracker",
            "https://tracker-a.example,https://tracker-b.example",
            "--token",
            "secret-token",
        ]);

        match app.cmd {
            Command::Init {
                trackers,
                tracker_token,
                ..
            } => {
                assert_eq!(
                    trackers,
                    vec![
                        "https://tracker-a.example".to_string(),
                        "https://tracker-b.example".to_string()
                    ]
                );
                assert_eq!(tracker_token.as_deref(), Some("secret-token"));
            }
            _ => panic!("expected init"),
        }
    }

    #[test]
    fn render_config_toml_includes_values() {
        let peer_id = libp2p::PeerId::random().to_string();
        let relay = format!("/ip4/127.0.0.1/tcp/4001/p2p/{peer_id}");

        let args = InitArgs {
            force: false,
            user_id: Some("u1".to_string()),
            device_id: Some("d1".to_string()),
            trackers: vec!["http://127.0.0.1:8850".to_string()],
            relay: Some(relay.clone()),
            tracker_token: Some("t1".to_string()),
        };

        let text = render_config_toml(&args, &config::FileConfig::default(), "/tmp/x.db").unwrap();
        assert!(text.contains("db_path"));
        assert!(text.contains("user_id = \"u1\""));
        assert!(text.contains("device_id = \"d1\""));
        assert!(text.contains("http://127.0.0.1:8850"));
        assert!(text.contains(&format!("relay_addr = {relay:?}")));
        assert!(text.contains("tracker_token = \"t1\""));
        assert!(text.contains("swarm_key_path"));
        assert!(text.contains("p2p_identity_key_path"));
        assert!(text.contains("p2p_request_attempts"));
        assert!(text.contains("record_ignore_regex"));
        assert!(text.contains("async_upload"));
        assert!(text.contains("auto_prune"));
    }

    #[test]
    fn resolve_tracker_token_rejects_control_characters() {
        let cfg = config::FileConfig::default();
        let err = resolve_tracker_token(Some("abc\nxyz".to_string()), &cfg).unwrap_err();
        assert!(format!("{err:#}").contains("must not contain control characters"));
    }

    #[test]
    fn record_ignore_regex_matches_command() {
        assert!(should_ignore_record_command("echo token=abc", "(?i)token").unwrap());
        assert!(!should_ignore_record_command("echo hello", "(?i)token").unwrap());
    }

    #[test]
    fn record_ignore_regex_invalid_pattern_is_error() {
        assert!(should_ignore_record_command("echo hello", "(").is_err());
    }

    #[test]
    fn import_parses_flags() {
        let app = App::parse_from([
            "rr", "import", "--shell", "bash", "--path", "/tmp/x", "--limit", "10",
        ]);
        match app.cmd {
            Command::Import {
                shell, path, limit, ..
            } => {
                assert_eq!(shell, "bash");
                assert_eq!(path.as_deref(), Some("/tmp/x"));
                assert_eq!(limit, Some(10));
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn import_accepts_hishtory_source() {
        let app = App::parse_from(["rr", "import", "--shell", "hishtory"]);
        match app.cmd {
            Command::Import { shell, .. } => {
                assert_eq!(shell, "hishtory");
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn cleanup_hishtory_parses_safety_flags() {
        let app = App::parse_from([
            "rr",
            "cleanup-hishtory",
            "--apply",
            "--archive-dir",
            "/tmp/hishtory-backups",
        ]);
        match app.cmd {
            Command::CleanupHishtory {
                apply,
                archive_dir,
                no_archive,
                ..
            } => {
                assert!(apply);
                assert_eq!(archive_dir.as_deref(), Some("/tmp/hishtory-backups"));
                assert!(!no_archive);
            }
            _ => panic!("expected cleanup-hishtory"),
        }
    }
}
