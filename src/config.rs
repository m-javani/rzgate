// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

// config.rs
use crate::error::RZError;
use serde::Deserialize;
use std::fs;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub enum Mode {
    #[serde(rename = "standalone")]
    Standalone,
    #[serde(rename = "router")]
    Router,
}

impl Default for Mode {
    fn default() -> Self {
        Mode::Standalone
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    // Connection settings
    pub addr: String,
    pub port: u16,
    pub mode: Mode,

    // HTTP settings
    #[serde(default = "default_listening_addr")]
    pub listening_addr: String,
    #[serde(default = "default_http_port")]
    pub http_port: u16,

    // Connection behavior
    #[serde(default = "default_timeout_sec")]
    pub timeout_sec: u64,
    #[serde(default = "default_keep_alive_sec")]
    pub keep_alive_sec: u64,
    #[serde(default = "default_conn_per_node")]
    pub conn_per_node: usize,
    #[serde(default = "default_max_active_conns")]
    pub max_active_conns: usize,
    #[serde(default = "default_worker_threads")]
    pub worker_threads: usize,
}

// Default value functions
fn default_timeout_sec() -> u64 {
    2
}
fn default_keep_alive_sec() -> u64 {
    30
}
fn default_conn_per_node() -> usize {
    10
}
fn default_max_active_conns() -> usize {
    10000
}
fn default_worker_threads() -> usize {
    num_cpus::get_physical() * 3
}
fn default_listening_addr() -> String {
    "0.0.0.0".into()
}
fn default_http_port() -> u16 {
    8777
}

impl Config {
    pub fn load(config_path: &str) -> Result<Self, RZError> {
        let config_content = fs::read_to_string(config_path).map_err(|e| {
            RZError::Config(format!("Failed to read config file {}: {}", config_path, e))
        })?;

        let config: Config = serde_yaml::from_str(&config_content)
            .map_err(|e| RZError::Config(format!("Failed to parse config YAML: {}", e)))?;

        // Validation
        if config.addr.trim().is_empty() {
            return Err(RZError::Config("addr is required".into()));
        }
        if config.port == 0 {
            return Err(RZError::Config("port is required".into()));
        }

        Ok(config)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1".into(),
            port: 7777,
            mode: Mode::Standalone,
            timeout_sec: default_timeout_sec(),
            keep_alive_sec: default_keep_alive_sec(),
            conn_per_node: default_conn_per_node(),
            max_active_conns: default_max_active_conns(),
            worker_threads: default_worker_threads(),
            listening_addr: default_listening_addr(),
            http_port: default_http_port(),
        }
    }
}
