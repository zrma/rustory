mod build_info;
mod cli;
mod config;
mod core;
mod daemon;
mod device_retirement;
mod hishtory_cleanup;
mod history_import;
mod hook;
mod http_retry;
mod libp2p;
mod managed_logs;
mod p2p;
mod p2p_codec;
mod runtime_tasks;
mod search;
mod self_update;
mod storage;
mod sync;
mod sync_status;
mod terminal;
mod tracker;
mod transport;
mod uninstall;
mod watch_tui;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
