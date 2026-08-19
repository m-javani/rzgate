// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzproxy.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use std::str::FromStr;

// config.rs
use clap::Parser;

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Standalone,
    Router,
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "standalone" => Ok(Mode::Standalone),
            "router" => Ok(Mode::Router),
            _ => Err(format!(
                "Invalid mode: {}. Must be 'standalone' or 'router'",
                s
            )),
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Standalone => write!(f, "standalone"),
            Mode::Router => write!(f, "router"),
        }
    }
}

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Config {
    /// Server mode: "standalone" or "router"
    #[clap(long, default_value = "standalone")]
    pub mode: Mode,

    /// Listening address for HTTP
    #[clap(long, default_value = "0.0.0.0")]
    pub listening_addr: String,

    /// HTTP port
    #[clap(long, default_value = "8777")]
    pub http_port: u16,

    /// Roomzin server address
    #[clap(long, default_value = "127.0.0.1")]
    pub roomzin_addr: String,

    /// Roomzin server port
    #[clap(long, default_value = "7777")]
    pub roomzin_port: u16,

    /// Request timeout in seconds
    #[clap(long, default_value = "2")]
    pub timeout_sec: u64,

    /// Keep-alive interval in seconds
    #[clap(long, default_value = "15")]
    pub keep_alive_sec: u64,

    /// Connections per node
    #[clap(long, default_value = "10")]
    pub conn_per_node: usize,

    /// Maximum active connections
    #[clap(long, default_value = "10000")]
    pub max_active_conns: usize,

    /// Worker threads (0 = auto = cores * 3)
    #[clap(long, default_value = "0")]
    pub worker_threads: usize,
}

impl Config {
    pub fn parse() -> Self {
        let mut config = <Self as Parser>::parse();
        config.roomzin_addr = config
            .roomzin_addr
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string();
        config
    }

    pub fn effective_workers(&self) -> usize {
        if self.worker_threads == 0 {
            num_cpus::get_physical() * 3
        } else {
            self.worker_threads
        }
    }

    pub fn keep_alive_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.keep_alive_sec)
    }
}
