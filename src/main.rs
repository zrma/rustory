mod cli;
mod config;
mod core;
mod hook;
mod http_retry;
mod p2p;
mod p2p_codec;
mod search;
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
