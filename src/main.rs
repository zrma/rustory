mod cli;
mod core;
mod hook;
mod storage;
mod sync;
mod transport;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
