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
use std::{fmt, fs, str::FromStr};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub enum DiscoveryKind {
    #[serde(rename = "static")]
    Static,
    #[serde(rename = "http")]
    Http,
}

impl Default for DiscoveryKind {
    fn default() -> Self {
        DiscoveryKind::Static
    }
}

impl fmt::Display for DiscoveryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscoveryKind::Static => write!(f, "static"),
            DiscoveryKind::Http => write!(f, "http"),
        }
    }
}

impl FromStr for DiscoveryKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "static" => Ok(DiscoveryKind::Static),
            "http" => Ok(DiscoveryKind::Http),
            _ => Err(format!("Invalid discovery kind: {}", s)),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub roomzin_seed_ids: String,
    pub roomzin_api_port: u16,
    pub roomzin_tcp_port: u16,
    #[serde(default)]
    pub conn_per_roomzin_node: usize,
    #[serde(default)]
    pub listening_addr: String,
    #[serde(default)]
    pub http_port: u16,
    #[serde(default)]
    pub https_port: u16,
    #[serde(default)]
    pub http_enabled: bool,
    #[serde(default)]
    pub https_enabled: bool,
    #[serde(default)]
    pub auth_enabled: bool,
    #[serde(default)]
    pub tokens_path: String,
    #[serde(default)]
    pub tls_cert_path: String,
    #[serde(default)]
    pub tls_key_path: String,
    #[serde(default)]
    pub timeout_sec: u64,
    #[serde(default)]
    pub http_timeout_sec: u64,
    #[serde(default)]
    pub keep_alive_sec: u64,
    #[serde(default)]
    pub node_probe_interval_sec: u64,
    #[serde(default)]
    pub max_active_conns: usize,
    #[serde(default)]
    pub worker_threads: usize,

    // Discovery related
    #[serde(default)]
    pub discovery_kind: DiscoveryKind,
    #[serde(default)]
    pub discovery_yml_path: Option<String>,
    #[serde(default)]
    pub discovery_addr: Option<String>,
    #[serde(skip)]
    pub discovery_refresh_interval_sec: u64,

    // New fields for standalone mode
    #[serde(default)]
    pub no_cluster: bool,
    #[serde(default)]
    pub roomzin_standalone_host: Option<String>,
}

impl Config {
    pub fn load(config_path: &str) -> Result<Self, RZError> {
        let config_content = fs::read_to_string(config_path).map_err(|e| {
            RZError::Config(format!("Failed to read config file {}: {}", config_path, e))
        })?;

        let mut config: Config = serde_yaml::from_str(&config_content)
            .map_err(|e| RZError::Config(format!("Failed to parse config YAML: {}", e)))?;

        // === Validation & Defaulting Logic ===

        if config.no_cluster {
            // Standalone mode validation
            if config.roomzin_standalone_host.is_none()
                || config
                    .roomzin_standalone_host
                    .as_ref()
                    .unwrap()
                    .trim()
                    .is_empty()
            {
                return Err(RZError::Config(
                    "roomzin_standalone_host is required when no_cluster = true".into(),
                ));
            }

            // Disable discovery in standalone mode
            config.discovery_kind = DiscoveryKind::Static; // doesn't matter
            config.discovery_yml_path = None;
            config.discovery_addr = None;
        } else {
            // Cluster mode validation (existing logic)
            match config.discovery_kind {
                DiscoveryKind::Static => {
                    if config.discovery_yml_path.is_none() {
                        config.discovery_yml_path = Some("./discovery.yml".into());
                    }
                    config.discovery_addr = None;
                }
                DiscoveryKind::Http => {
                    if config.discovery_addr.is_none()
                        || config.discovery_addr.as_ref().unwrap().trim().is_empty()
                    {
                        return Err(RZError::Config(
                            "discovery_addr must be provided when discovery_kind is http".into(),
                        ));
                    }
                    config.discovery_yml_path = None;
                }
            }
        }

        // General defaults
        if config.timeout_sec == 0 {
            config.timeout_sec = 2;
        }
        if config.http_timeout_sec == 0 {
            config.http_timeout_sec = 2;
        }
        if config.keep_alive_sec == 0 {
            config.keep_alive_sec = 30;
        }
        if config.node_probe_interval_sec == 0 {
            config.node_probe_interval_sec = 2;
        }
        if config.max_active_conns == 0 {
            config.max_active_conns = 10_000;
        }
        if config.roomzin_api_port == 0 {
            config.roomzin_api_port = 8080;
        }
        if config.worker_threads == 0 {
            config.worker_threads = num_cpus::get_physical() * 3;
        }
        if config.conn_per_roomzin_node == 0 {
            config.conn_per_roomzin_node = 1;
        }
        if config.https_port == 0 {
            config.https_port = 3443;
        }
        if config.http_port == 0 {
            config.http_port = 8777;
        }
        if config.listening_addr.trim().is_empty() {
            config.listening_addr = "0.0.0.0".into();
        }

        Ok(config)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listening_addr: "0.0.0.0".into(),
            http_port: 8777,
            https_port: 3443,
            http_enabled: true,
            https_enabled: false,
            auth_enabled: false,
            roomzin_api_port: 8080,
            timeout_sec: 2,
            http_timeout_sec: 2,
            keep_alive_sec: 30,
            node_probe_interval_sec: 2,
            max_active_conns: 10_000,
            roomzin_seed_ids: "".into(),
            roomzin_tcp_port: 7777,
            tokens_path: "./auth.yml".into(),
            tls_cert_path: "./cert.pem".into(),
            tls_key_path: "./key.pem".into(),
            worker_threads: 0,
            conn_per_roomzin_node: 1,
            discovery_kind: DiscoveryKind::Static,
            discovery_yml_path: Some("./discovery.yml".into()),
            discovery_addr: None,
            discovery_refresh_interval_sec: 2,
            no_cluster: false,
            roomzin_standalone_host: None,
        }
    }
}
