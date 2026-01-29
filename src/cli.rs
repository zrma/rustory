use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{hook, storage, transport};

#[derive(Parser)]
#[command(name = "rr", version, about = "Rustory CLI")]
pub struct App {
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

    match app.cmd {
        Command::Serve { bind } => {
            transport::serve(&bind)?;
        }
        Command::Sync { peers } => {
            transport::sync(&peers)?;
        }
        Command::Search { limit } => {
            let store = storage::LocalStore::open("~/.rustory/history.db")?;
            let _entries = store.list_recent(limit)?;
        }
        Command::Hook { shell } => {
            let shell = hook::Shell::parse(shell.as_str())?;
            let content = hook::render_hook(shell);
            println!("{content}");
        }
    }

    Ok(())
}
