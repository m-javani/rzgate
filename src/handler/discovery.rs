// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use arc_swap::ArcSwap;
use futures::future::join_all;
use reqwest::{Client, header};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::auth;
use crate::config::Config;
use crate::error::RZError;

#[derive(Clone, Debug, Deserialize)]
pub struct NodeAddr {
    pub node_id: String,
    pub addr: String,
    pub port: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DiscoveryYaml {
    pub nodes: Vec<NodeAddr>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NodeInfo {
    #[serde(rename = "node_id")]
    pub node_id: String,
    #[serde(rename = "zone_id")]
    pub zone_id: String,
    #[serde(rename = "shard_id")]
    pub shard_id: String,
    #[serde(rename = "leader_id")]
    pub leader_id: String,
    #[serde(rename = "leader_url")]
    pub leader_url: String,
}

fn split_ids(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

async fn http_get<T: for<'de> Deserialize<'de>>(
    client: &Client,
    host: &str,
    port: u32,
    path: &str,
    auth_token: &str,
) -> Result<T, RZError> {
    let url = format!("http://{host}:{port}{path}");

    let mut req = client.get(&url);
    if !auth_token.is_empty() {
        req = req.header(header::AUTHORIZATION, format!("Bearer {auth_token}"));
    }

    let resp = req.send().await?;

    if !resp.status().is_success() {
        return Err(RZError::Http(resp.status().to_string()));
    }

    Ok(resp.json().await?)
}

async fn get_node_info(
    client: &Client,
    host: &str,
    port: u32,
    auth_token: &str,
) -> Result<NodeInfo, RZError> {
    http_get(client, host, port, "/node-info", auth_token).await
}

async fn health_check(
    client: &Client,
    host: &str,
    api_port: u32,
    auth_token: &str,
) -> Result<String, RZError> {
    let url = format!("http://{host}:{api_port}/healthz");

    let mut req = client.get(&url);
    if !auth_token.is_empty() {
        req = req.header(header::AUTHORIZATION, format!("Bearer {auth_token}"));
    }

    let resp = req.send().await?;

    if resp.status().as_u16() != 200 {
        return Err(RZError::Http(resp.status().to_string()));
    }

    let body = resp.text().await?;
    Ok(body.trim().to_string())
}

#[derive(Debug)]
struct NodeData {
    host: String,
    health: String,
    leader_url: String,
}

pub async fn get_cluster_info(
    cfg: &Config,
    discovery_map: Arc<ArcSwap<HashMap<String, (String, Option<u32>)>>>,
) -> Result<(String, Vec<String>), RZError> {
    let node_ids = split_ids(&cfg.roomzin_seed_ids);
    let http_timeout = Duration::from_secs(cfg.http_timeout_sec);

    let client = Client::builder().timeout(http_timeout).build()?;

    let nodes = Arc::new(Mutex::new(HashMap::<String, NodeData>::new()));
    let existing: HashSet<String> = node_ids.iter().cloned().collect();
    let discovered = Arc::new(Mutex::new(HashSet::<String>::new()));

    // First phase: query seed hosts
    let mut tasks = Vec::new();
    for node_id in node_ids {
        let client = client.clone();
        let node_id = node_id.clone();
        let nodes = nodes.clone();
        let discovered = discovered.clone();
        let existing = existing.clone();
        let discovery_map = discovery_map.clone();

        let roomzin_port = cfg.roomzin_api_port.clone() as u32;
        let task = tokio::spawn(async move {
            // Get address from discovery map
            let (host, p) = match discovery_map.load().get(&node_id) {
                Some(a) => a.clone(),
                None => {
                    // tracing::warn!("No address found for node: {}", node_id);
                    return;
                }
            };
            let p = p.unwrap_or(roomzin_port);

            let auth_token = auth::get_roomzin_token();
            let health = match health_check(&client, &host, p, &auth_token).await {
                Ok(h) if h != "unavailable" => h,
                _ => return,
            };

            let info = match get_node_info(&client, &host, p, &auth_token).await {
                Ok(i) => i,
                Err(_) => return,
            };

            {
                let mut nodes_lock = nodes.lock().await;
                nodes_lock.insert(
                    host.clone(),
                    NodeData {
                        host: host.clone(),
                        health: health.clone(),
                        leader_url: info.leader_url.clone(),
                    },
                );
            }

            let peers: Result<Vec<String>, _> =
                http_get(&client, &host, p, "/peers", &auth_token).await;
            if let Ok(peers) = peers {
                let mut discovered_lock = discovered.lock().await;
                for peer in peers {
                    if !existing.contains(&peer) {
                        discovered_lock.insert(peer);
                    }
                }
            }
        });

        tasks.push(task);
    }

    join_all(tasks).await;

    // Second phase: query discovered nodes
    let new_node_ids: Vec<String> = {
        let discovered_lock = discovered.lock().await;
        discovered_lock.iter().cloned().collect()
    };

    let mut tasks = Vec::new();
    for node_id in new_node_ids {
        let client = client.clone();
        let node_id = node_id.clone();
        let nodes = nodes.clone();
        let discovery_map = discovery_map.clone();

        let roomzin_port = cfg.roomzin_api_port.clone() as u32;
        let task = tokio::spawn(async move {
            let (host, p) = match discovery_map.load().get(&node_id) {
                Some(a) => a.clone(),
                None => {
                    // tracing::warn!("No address found for node: {}", node_id);
                    return;
                }
            };
            let p = p.unwrap_or(roomzin_port);

            let auth_token = auth::get_roomzin_token();
            let health = match health_check(&client, &host, p, &auth_token).await {
                Ok(h) if h != "unavailable" => h,
                _ => return,
            };

            let info = match get_node_info(&client, &host, p, &auth_token).await {
                Ok(i) => i,
                Err(_) => return,
            };

            let mut nodes_lock = nodes.lock().await;
            nodes_lock.insert(
                host.clone(),
                NodeData {
                    host: host.clone(),
                    health,
                    leader_url: info.leader_url,
                },
            );
        });

        tasks.push(task);
    }

    join_all(tasks).await;

    // Voting logic remains the same
    let nodes_guard = nodes.lock().await;
    let mut votes: HashMap<String, usize> = HashMap::new();

    for node in nodes_guard.values() {
        if !node.leader_url.is_empty() {
            *votes.entry(node.leader_url.clone()).or_insert(0) += 1;
        }
    }

    let leader_url = votes
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|(url, _)| url)
        .ok_or(RZError::NoLeaderAvailable)?;

    let mut leader_host = None;
    let mut followers = Vec::new();

    for node in nodes_guard.values() {
        if node.leader_url == leader_url {
            match node.health.as_str() {
                "active_leader" => leader_host = Some(node.host.clone()),
                "active_follower" => followers.push(node.host.clone()),
                _ => {}
            }
        }
    }

    let leader = leader_host.ok_or(RZError::NoLeaderAvailable)?;
    tracing::info!("leader: {:#?}", leader);
    tracing::info!("followers: {:#?}", followers);

    Ok((leader, followers))
}

pub fn load_discovery_nodes(yml_path: &str) -> Result<Vec<NodeAddr>, RZError> {
    let content = fs::read_to_string(yml_path).map_err(|e| {
        RZError::Config(format!("Failed to read discovery file {}: {}", yml_path, e))
    })?;

    let discovery: DiscoveryYaml = serde_yaml::from_str(&content)
        .map_err(|e| RZError::Config(format!("Failed to parse discovery YAML: {}", e)))?;

    if discovery.nodes.is_empty() {
        return Err(RZError::Config("Discovery nodes list is empty".to_string()));
    }

    tracing::info!("Loaded {} discovery nodes", discovery.nodes.len());

    Ok(discovery.nodes)
}
