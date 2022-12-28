// startup.rs
#![allow(dead_code)]

use log::*;
use std::fmt::Debug;
use structopt::StructOpt;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone, Debug, StructOpt)]
pub struct OptsCommon {
    #[structopt(short, long)]
    pub debug: bool,
    #[structopt(short, long)]
    pub trace: bool,
    #[structopt(short, long, default_value = "127.0.0.1:8080")]
    pub listen: String,
    #[structopt(short, long, default_value = "coap://127.0.0.1/")]
    pub coap_url: String,
}
impl OptsCommon {
    pub fn finish(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    pub fn get_loglevel(&self) -> LevelFilter {
        if self.trace {
            LevelFilter::Trace
        } else if self.debug {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        }
    }
}

pub fn start_pgm(_c: &OptsCommon, desc: &str) {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "pwr_server=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting up {desc}...");
    debug!("Git branch: {}", env!("GIT_BRANCH"));
    debug!("Git commit: {}", env!("GIT_COMMIT"));
    debug!("Source timestamp: {}", env!("SOURCE_TIMESTAMP"));
    debug!("Compiler version: {}", env!("RUSTC_VERSION"));
}

// EOF
