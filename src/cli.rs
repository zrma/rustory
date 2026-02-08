use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rand::Rng;

use crate::{config, hook, p2p, search, storage, tracker, transport};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "rr", version, about = "Rustory CLI")]
pub struct App {
    #[arg(long, global = true)]
    db_path: Option<String>,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long, default_value = "0.0.0.0:8844")]
        bind: String,
    },
    Sync {
        #[arg(long, value_delimiter = ',')]
        peers: Vec<String>,

        #[arg(long)]
        push: bool,
    },
    P2pServe {
        #[arg(long, default_value = "/ip4/0.0.0.0/tcp/0")]
        listen: String,

        #[arg(long)]
        identity_key: Option<String>,

        #[arg(long)]
        swarm_key: Option<String>,

        #[arg(long)]
        relay: Option<String>,

        #[arg(long, value_delimiter = ',')]
        trackers: Vec<String>,

        #[arg(long)]
        tracker_token: Option<String>,
    },
    P2pSync {
        #[arg(long, value_delimiter = ',')]
        peers: Vec<String>,

        #[arg(long, default_value_t = 1000)]
        limit: usize,

        #[arg(long)]
        push: bool,

        #[arg(long)]
        watch: bool,

        #[arg(long, default_value_t = 60)]
        interval_sec: u64,

        #[arg(long)]
        start_jitter_sec: Option<u64>,

        #[arg(long)]
        swarm_key: Option<String>,

        #[arg(long)]
        relay: Option<String>,

        #[arg(long, value_delimiter = ',')]
        trackers: Vec<String>,

        #[arg(long)]
        tracker_token: Option<String>,
    },
    SwarmKey {
        #[arg(long)]
        swarm_key: Option<String>,
    },
    Record {
        #[arg(long)]
        cmd: String,

        #[arg(long)]
        cwd: Option<String>,

        #[arg(long, default_value_t = 0)]
        exit_code: i32,

        #[arg(long, default_value_t = 0)]
        duration_ms: i64,

        #[arg(long)]
        shell: Option<String>,

        #[arg(long)]
        hostname: Option<String>,

        #[arg(long)]
        user_id: Option<String>,

        #[arg(long)]
        device_id: Option<String>,

        #[arg(long, default_value_t = false)]
        print_id: bool,
    },
    Search {
        #[arg(long)]
        limit: Option<usize>,
    },
    Hook {
        #[arg(long, default_value = "zsh")]
        shell: String,
    },
    TrackerServe {
        #[arg(long, default_value = "0.0.0.0:8850")]
        bind: String,

        #[arg(long, default_value_t = 60)]
        ttl_sec: u64,

        #[arg(long)]
        token: Option<String>,
    },
    RelayServe {
        #[arg(long, default_value = "/ip4/0.0.0.0/tcp/4001")]
        listen: String,

        #[arg(long)]
        identity_key: Option<String>,

        #[arg(long)]
        swarm_key: Option<String>,
    },
    Doctor {},
}

pub fn run() -> Result<()> {
    let app = App::parse();
    let cfg = config::load_default()?;

    let db_path = normalize_opt_string(app.db_path)
        .or_else(|| env_nonempty("RUSTORY_DB_PATH"))
        .or_else(|| normalize_opt_string(cfg.db_path.clone()))
        .unwrap_or_else(|| storage::DEFAULT_DB_PATH.to_string());

    match app.cmd {
        Command::Serve { bind } => {
            transport::serve(&bind, &db_path)?;
        }
        Command::Sync { peers, push } => {
            let device_id = resolve_device_id(&cfg);
            transport::sync(&peers, &db_path, push, Some(&device_id))?;
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
            swarm_key,
            relay,
            trackers,
            tracker_token,
        } => {
            let psk = resolve_swarm_psk(swarm_key, &cfg)?;
            let relay_addr = resolve_relay_addr(relay, &cfg)?;
            let trackers = resolve_trackers(trackers, &cfg)?;
            let tracker_token = resolve_tracker_token(tracker_token, &cfg)?;
            let user_id = resolve_user_id(&cfg);
            let device_id = resolve_device_id(&cfg);

            let sync_cfg = p2p::SyncConfig {
                psk,
                relay_addr,
                trackers,
                tracker_token,
                user_id: Some(user_id),
                device_id: Some(device_id),
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
                    let delay = rand::thread_rng().gen_range(0..=start_jitter_sec);
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
            if is_self_rr_command(cmd) {
                return Ok(());
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
        }
        Command::Search { limit } => {
            let limit = resolve_search_limit(limit, &cfg)?;

            let store = storage::LocalStore::open(&db_path)?;
            let entries = store.list_recent(limit)?;
            if let Some(cmd) = search::select_command(&entries)? {
                println!("{cmd}");
            }
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
        } => {
            tracker::serve(&bind, ttl_sec, token)?;
        }
        Command::RelayServe {
            listen,
            identity_key,
            swarm_key,
        } => {
            let psk = resolve_swarm_psk(swarm_key, &cfg)?;
            let identity = resolve_relay_identity(identity_key, &cfg)?;
            p2p::relay_serve(&listen, p2p::RelayServeConfig { identity, psk })?;
        }
        Command::Doctor {} => {
            run_doctor(&cfg, &db_path)?;
        }
    }

    Ok(())
}

fn run_doctor(cfg: &config::FileConfig, db_path: &str) -> Result<()> {
    use std::path::Path;

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

    let db_path_expanded = if db_path == ":memory:" {
        Path::new(":memory:").to_path_buf()
    } else {
        config::expand_home_path(db_path)?
    };

    let user_id = resolve_user_id(cfg);
    let device_id = resolve_device_id(cfg);

    println!("config path: {} (exists: {cfg_exists})", cfg_path.display());
    println!("db path: {}", db_path_expanded.display());
    println!("user_id: {user_id}");
    println!("device_id: {device_id}");

    let swarm_key_path = resolve_swarm_key_path(None, cfg);
    let swarm_fp = config::load_swarm_key(&swarm_key_path)?.map(|k| k.fingerprint().to_string());
    print_key_status("swarm key", &swarm_key_path, swarm_fp.as_deref())?;

    let p2p_identity_key_path = resolve_p2p_identity_key_path(None, cfg);
    let p2p_peer_id = config::load_identity_keypair(&p2p_identity_key_path)?
        .map(|kp| kp.public().to_peer_id().to_string());
    print_key_status(
        "p2p identity key",
        &p2p_identity_key_path,
        p2p_peer_id.as_deref(),
    )?;

    let relay_identity_key_path = resolve_relay_identity_key_path(None, cfg);
    let relay_peer_id = config::load_identity_keypair(&relay_identity_key_path)?
        .map(|kp| kp.public().to_peer_id().to_string());
    print_key_status(
        "relay identity key",
        &relay_identity_key_path,
        relay_peer_id.as_deref(),
    )?;

    match resolve_relay_addr(None, cfg) {
        Ok(Some(addr)) => println!("relay addr: {addr}"),
        Ok(None) => println!("relay addr: (none)"),
        Err(err) => println!("relay addr: invalid: {err:#}"),
    }

    let trackers = resolve_trackers(Vec::new(), cfg)?;
    if trackers.is_empty() {
        println!("trackers: (none)");
        return Ok(());
    }

    let token = resolve_tracker_token(None, cfg)?;
    println!("trackers:");
    for base_url in trackers {
        let ping = tracker_ping(&base_url, token.as_deref());
        match ping {
            Ok(()) => println!("- {base_url} (ping: ok)"),
            Err(err) => println!("- {base_url} (ping: fail: {err})"),
        }
    }

    Ok(())
}

fn print_key_status(label: &str, path: &str, extra: Option<&str>) -> Result<()> {
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

    let mut suffix = String::new();
    if let Some(extra) = extra {
        suffix.push_str(&format!(" {extra}"));
    }

    if let Some(mode) = file_mode_777(&expanded)
        && mode != 0o600
    {
        suffix.push_str(&format!(" (warn: mode={mode:03o}, want 600)"));
    }

    println!("{label}: {} (exists){suffix}", expanded.display());
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

fn tracker_ping(base_url: &str, token: Option<&str>) -> std::result::Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(1))
        .timeout_read(Duration::from_secs(1))
        .timeout_write(Duration::from_secs(1))
        .build();

    let url = format!("{}/api/v1/ping", base_url.trim_end_matches('/'));
    let mut req = agent.get(&url);
    if let Some(token) = token {
        req = req.set("Authorization", &format!("Bearer {}", token.trim()));
    }

    match req.call() {
        Ok(resp) => {
            if resp.status() == 200 {
                Ok(())
            } else {
                Err(format!("status {}", resp.status()))
            }
        }
        Err(err) => Err(err.to_string()),
    }
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

fn resolve_search_limit(cli: Option<usize>, cfg: &config::FileConfig) -> Result<usize> {
    if let Some(v) = cli {
        return Ok(v);
    }

    if let Some(v) = env_nonempty("RUSTORY_SEARCH_LIMIT") {
        let parsed: usize = v
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid RUSTORY_SEARCH_LIMIT={:?}: {e}", v.trim()))?;
        return Ok(parsed);
    }

    if let Some(v) = cfg.search_limit_default {
        return Ok(v);
    }

    Ok(100000)
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

fn resolve_tracker_token(cli: Option<String>, cfg: &config::FileConfig) -> Result<Option<String>> {
    Ok(normalize_opt_string(cli)
        .or_else(|| env_nonempty("RUSTORY_TRACKER_TOKEN"))
        .or_else(|| normalize_opt_string(cfg.tracker_token.clone())))
}

fn resolve_peer_meta(cfg: &config::FileConfig) -> crate::tracker::PeerMeta {
    let hostname = env_nonempty("HOSTNAME").unwrap_or_else(|| "unknown".to_string());
    let user_id = resolve_user_id(cfg);
    let device_id = resolve_device_id(cfg);

    crate::tracker::PeerMeta {
        device_id: Some(device_id),
        hostname: Some(hostname),
        user_id: Some(user_id),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
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
        .unwrap_or_else(|| env_nonempty("HOSTNAME").unwrap_or_else(|| "unknown".to_string()))
}

fn is_self_rr_command(cmd: &str) -> bool {
    let Some(first) = cmd.split_whitespace().next() else {
        return false;
    };
    first == "rr"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_self_rr_command_detects_rr_invocation() {
        assert!(is_self_rr_command("rr"));
        assert!(is_self_rr_command("rr serve --bind 0.0.0.0:8844"));
        assert!(is_self_rr_command("  rr  search"));

        assert!(!is_self_rr_command(""));
        assert!(!is_self_rr_command("echo rr"));
        assert!(!is_self_rr_command("rrr search"));
        assert!(!is_self_rr_command("cargo run --bin rr -- serve"));
    }

    #[test]
    fn p2p_sync_watch_parses_flags() {
        let app = App::parse_from(["rr", "p2p-sync", "--watch", "--interval-sec", "5"]);
        match app.cmd {
            Command::P2pSync {
                watch,
                interval_sec,
                start_jitter_sec,
                ..
            } => {
                assert!(watch);
                assert_eq!(interval_sec, 5);
                assert!(start_jitter_sec.is_none());
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
    fn doctor_parses() {
        let app = App::parse_from(["rr", "doctor"]);
        match app.cmd {
            Command::Doctor {} => {}
            _ => panic!("expected doctor"),
        }
    }
}
