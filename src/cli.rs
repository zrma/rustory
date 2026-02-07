use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{hook, p2p, search, storage, transport};

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
    },
    P2pServe {
        #[arg(long, default_value = "/ip4/0.0.0.0/tcp/0")]
        listen: String,
    },
    P2pSync {
        #[arg(long, value_delimiter = ',')]
        peers: Vec<String>,

        #[arg(long, default_value_t = 1000)]
        limit: usize,
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
        #[arg(long, default_value_t = 100000)]
        limit: usize,
    },
    Hook {
        #[arg(long, default_value = "zsh")]
        shell: String,
    },
}

pub fn run() -> Result<()> {
    let app = App::parse();
    let db_path =
        normalize_opt_string(app.db_path).unwrap_or_else(|| storage::DEFAULT_DB_PATH.to_string());

    match app.cmd {
        Command::Serve { bind } => {
            transport::serve(&bind, &db_path)?;
        }
        Command::Sync { peers } => {
            transport::sync(&peers, &db_path)?;
        }
        Command::P2pServe { listen } => {
            p2p::serve(&listen, &db_path)?;
        }
        Command::P2pSync { peers, limit } => {
            p2p::sync(&peers, limit, &db_path)?;
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
            if cmd.trim().is_empty() {
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
                .or_else(|| env_nonempty("USER"))
                .unwrap_or_else(|| "unknown".to_string());

            let device_id = normalize_opt_string(device_id)
                .or_else(|| env_nonempty("RUSTORY_DEVICE_ID"))
                .unwrap_or_else(|| hostname.clone());

            let entry = crate::core::Entry::new(crate::core::EntryInput {
                device_id,
                user_id,
                ts: time::OffsetDateTime::now_utc(),
                cmd,
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
    }

    Ok(())
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
