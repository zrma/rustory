mod build_info;
mod cli;
mod config;
mod core;
mod hishtory_cleanup;
mod history_import;
mod hook;
mod http_retry;
mod libp2p;
mod p2p;
mod p2p_codec;
mod search;
mod self_update;
mod storage;
mod sync;
mod tracker;
mod transport;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
