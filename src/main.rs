use std::{
    io::{self, IsTerminal},
    process::ExitCode,
};

use rudis::Server;
use tracing::error;
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

const ADDR: &str = "127.0.0.1:6379";

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(addr = ADDR, error = %e, "server failed");
            ExitCode::FAILURE
        },
    }
}

async fn run() -> io::Result<()> {
    Server::bind(ADDR).await?.run().await
}

fn init_tracing() {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .init();
}
