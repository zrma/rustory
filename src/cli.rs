use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rand::Rng;

use crate::{config, history_import, hook, p2p, search, storage, tracker, transport};
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
        req_attempts: Option<u64>,

        #[arg(long)]
        req_timeout_base_sec: Option<u64>,

        #[arg(long)]
        req_timeout_cap_sec: Option<u64>,

        #[arg(long)]
        req_backoff_base_ms: Option<u64>,

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
    SyncStatus {
        #[arg(long)]
        peer: Option<String>,

        #[arg(long, default_value_t = false)]
        json: bool,
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
    Init {
        #[arg(long)]
        force: bool,

        #[arg(long)]
        user_id: Option<String>,

        #[arg(long)]
        device_id: Option<String>,

        #[arg(long, value_delimiter = ',')]
        trackers: Vec<String>,

        #[arg(long)]
        relay: Option<String>,

        #[arg(long)]
        tracker_token: Option<String>,
    },
    Doctor {},
    Import {
        #[arg(long, default_value = "zsh")]
        shell: String,

        #[arg(long)]
        path: Option<String>,

        #[arg(long)]
        limit: Option<usize>,

        #[arg(long)]
        user_id: Option<String>,

        #[arg(long)]
        device_id: Option<String>,

        #[arg(long)]
        hostname: Option<String>,
    },
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
            req_attempts,
            req_timeout_base_sec,
            req_timeout_cap_sec,
            req_backoff_base_ms,
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
            let request_retry_policy = resolve_p2p_request_retry_policy(
                req_attempts,
                req_timeout_base_sec,
                req_timeout_cap_sec,
                req_backoff_base_ms,
                &cfg,
            )?;

            let sync_cfg = p2p::SyncConfig {
                psk,
                relay_addr,
                trackers,
                tracker_token,
                user_id: Some(user_id),
                device_id: Some(device_id),
                request_retry_policy,
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
        }
        Command::Search { limit } => {
            let limit = resolve_search_limit(limit, &cfg)?;

            let store = storage::LocalStore::open(&db_path)?;
            let entries = store.list_recent(limit)?;
            if let Some(cmd) = search::select_command(&entries)? {
                println!("{cmd}");
            }
        }
        Command::SyncStatus { peer, json } => {
            let peer = normalize_opt_string(peer);
            let store = storage::LocalStore::open(&db_path)?;
            let local_device_id = resolve_device_id(&cfg);
            let report = build_sync_status_report(&store, &local_device_id, peer.as_deref())?;

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
                return Ok(());
            }

            for status in report.peers {
                let last_seen = status
                    .last_seen_unix
                    .map(|ts| ts.to_string())
                    .unwrap_or_else(|| "-".to_string());
                println!(
                    "peer={} pull_cursor={} push_cursor={} pending_push={} last_seen_unix={}",
                    status.peer_id,
                    status.pull_cursor,
                    status.push_cursor,
                    status.pending_push,
                    last_seen
                );
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
        Command::Init {
            force,
            user_id,
            device_id,
            trackers,
            relay,
            tracker_token,
        } => {
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
        Command::Doctor {} => {
            run_doctor(&cfg, &db_path)?;
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

            let content = history_import::read_history_file(&path)?;
            let store = storage::LocalStore::open(&db_path)?;
            let stats = history_import::import_into_store(
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
            )?;

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
    }

    Ok(())
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
    println!("- p2p 동기화: rr p2p-sync --trackers <tracker> --relay <relay> --watch --push");

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
    out.push_str("# record_ignore_regex = \"(?i)(password|token|secret)\" # optional\n");

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

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct SyncStatusPeerReport {
    peer_id: String,
    pull_cursor: i64,
    push_cursor: i64,
    pending_push: usize,
    last_seen_unix: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct SyncStatusReport {
    local_head: i64,
    local_device_id: String,
    peers: Vec<SyncStatusPeerReport>,
}

fn build_sync_status_report(
    store: &storage::LocalStore,
    local_device_id: &str,
    peer_filter: Option<&str>,
) -> Result<SyncStatusReport> {
    let local_head = store.latest_ingest_seq()?;
    let peer_last_seen = store.list_peer_book_last_seen_map()?;
    let mut statuses = store.list_peer_sync_status()?;
    if let Some(peer_id) = peer_filter {
        statuses.retain(|status| status.peer_id == peer_id);
    }

    let mut peers = Vec::with_capacity(statuses.len());
    for status in statuses {
        let peer_id = status.peer_id;
        let pending_push = store.count_pending_push_entries(&peer_id, Some(local_device_id))?;
        let last_seen_unix = peer_last_seen.get(&peer_id).copied();
        peers.push(SyncStatusPeerReport {
            peer_id,
            pull_cursor: status.last_cursor,
            push_cursor: status.last_pushed_seq,
            pending_push,
            last_seen_unix,
        });
    }

    Ok(SyncStatusReport {
        local_head,
        local_device_id: local_device_id.to_string(),
        peers,
    })
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

fn should_ignore_record_command(
    cmd: &str,
    pattern: &str,
) -> std::result::Result<bool, regex::Error> {
    let re = regex::Regex::new(pattern)?;
    Ok(re.is_match(cmd))
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
    fn doctor_parses() {
        let app = App::parse_from(["rr", "doctor"]);
        match app.cmd {
            Command::Doctor {} => {}
            _ => panic!("expected doctor"),
        }
    }

    #[test]
    fn sync_status_parses_peer_filter() {
        let app = App::parse_from(["rr", "sync-status", "--peer", "peer-a"]);
        match app.cmd {
            Command::SyncStatus { peer, json } => {
                assert_eq!(peer.as_deref(), Some("peer-a"));
                assert!(!json);
            }
            _ => panic!("expected sync-status"),
        }
    }

    #[test]
    fn sync_status_parses_json_flag() {
        let app = App::parse_from(["rr", "sync-status", "--json"]);
        match app.cmd {
            Command::SyncStatus { peer, json } => {
                assert!(peer.is_none());
                assert!(json);
            }
            _ => panic!("expected sync-status"),
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
                version: "0.1.0".to_string(),
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
        store
            .upsert_peer_book(&storage::PeerBookPeer {
                peer_id: "peer-a".to_string(),
                addrs: vec!["/ip4/127.0.0.1/tcp/1111/p2p/peer-a".to_string()],
                user_id: Some("user1".to_string()),
                device_id: Some("dev-remote".to_string()),
                last_seen_unix: 99,
            })
            .unwrap();

        let report = build_sync_status_report(&store, "dev-local", None).unwrap();
        assert_eq!(report.local_head, 3);
        assert_eq!(report.local_device_id, "dev-local");
        assert_eq!(report.peers.len(), 2);

        let peer_a = report
            .peers
            .iter()
            .find(|peer| peer.peer_id == "peer-a")
            .unwrap();
        assert_eq!(peer_a.pull_cursor, 2);
        assert_eq!(peer_a.push_cursor, 1);
        assert_eq!(peer_a.pending_push, 1);
        assert_eq!(peer_a.last_seen_unix, Some(99));

        let peer_b = report
            .peers
            .iter()
            .find(|peer| peer.peer_id == "peer-b")
            .unwrap();
        assert_eq!(peer_b.pending_push, 0);
        assert_eq!(peer_b.last_seen_unix, None);

        let filtered = build_sync_status_report(&store, "dev-local", Some("peer-a")).unwrap();
        assert_eq!(filtered.peers.len(), 1);
        assert_eq!(filtered.peers[0].peer_id, "peer-a");

        let json = serde_json::to_string(&filtered).unwrap();
        assert!(json.contains("\"local_head\""));
        assert!(json.contains("\"local_device_id\""));
        assert!(json.contains("\"pending_push\""));
        assert!(json.contains("\"last_seen_unix\""));
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
}
