// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use arc_swap::ArcSwap;
use bytes::Bytes;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, RwLock, oneshot};
use tokio::time::{interval, sleep};
use tokio_util::sync::CancellationToken;

use crate::config::{Config, DiscoveryKind};
use crate::error::RZError;
use crate::handler::connection::Connection;
use crate::handler::demux::DemuxMap;
use crate::handler::discovery::{DiscoveryYaml, NodeAddr, get_cluster_info, load_discovery_nodes};
use crate::metrics::MetricsEvent;

#[derive(Clone)]
pub struct ClusterHandler {
    inner: Arc<HandlerInner>,
}

#[derive(Default)]
struct LoadBalancer {
    follower_history: HashMap<(String, u32), u32>, // node id, conn id
    last_node_id: u32,
    last_leader_id: u32,
}

struct HandlerInner {
    cfg: Config,
    pub leader_conns: Arc<RwLock<Vec<Option<Connection>>>>,
    pub leader_addr: ArcSwap<Option<String>>,
    followers: Arc<RwLock<HashMap<(String, u32, u32), Connection>>>, // node_name, node idx, conn idx
    metrics_tx: Sender<MetricsEvent>,
    load_balancer: Arc<Mutex<LoadBalancer>>, // node_id, conn_id
    cancel_token: CancellationToken,
    discovery_map: Arc<ArcSwap<HashMap<String, (String, Option<u32>)>>>, // node_id to (addr, port)
}

impl ClusterHandler {
    pub fn new(
        cfg: Config,
        metrics_tx: Sender<MetricsEvent>,
        cancel_token: CancellationToken,
    ) -> Arc<Self> {
        let conn_per_roomzin_node = cfg.conn_per_roomzin_node.clone();
        let leader_vec: Vec<Option<Connection>> = vec![None; conn_per_roomzin_node];

        // Initialize discovery map
        let discovery_map: ArcSwap<HashMap<String, (String, Option<u32>)>> =
            ArcSwap::new(Arc::new(HashMap::new()));

        // Load discovery nodes based on kind
        match cfg.discovery_kind {
            DiscoveryKind::Static => {
                if let Some(path) = &cfg.discovery_yml_path {
                    match load_discovery_nodes(path) {
                        Ok(nodes) => {
                            let mut map = HashMap::new();
                            for mut node in nodes {
                                if node.port.is_none() {
                                    node.port = Some(cfg.roomzin_api_port as u32);
                                }
                                map.insert(node.node_id, (node.addr, node.port));
                            }
                            tracing::info!("Loaded {} static discovery nodes", map.len());
                            discovery_map.store(Arc::new(map));
                        }
                        Err(e) => {
                            tracing::error!("Failed to load static discovery nodes: {}", e);
                        }
                    }
                }
            }
            DiscoveryKind::Http => {
                // Start with empty map, periodic task will update it
                tracing::info!(
                    "HTTP discovery mode - will fetch from {}",
                    cfg.discovery_addr
                        .as_ref()
                        .unwrap_or(&"<not set>".to_string())
                );
            }
        }

        let handler = Arc::new(Self {
            inner: Arc::new(HandlerInner {
                cfg: cfg.clone(),
                leader_conns: Arc::new(RwLock::new(leader_vec)),
                leader_addr: ArcSwap::new(Arc::new(None)),
                followers: Arc::new(RwLock::new(HashMap::new())),
                metrics_tx: metrics_tx.clone(),
                load_balancer: Arc::new(Mutex::new(LoadBalancer::default())),
                cancel_token: cancel_token.clone(),
                discovery_map: Arc::new(discovery_map),
            }),
        });

        // Spawn discovery updater task if HTTP mode
        if matches!(cfg.discovery_kind, DiscoveryKind::Http) {
            let h = handler.clone();
            tokio::spawn(async move {
                h.external_discovery_task().await;
            });
        }

        let h = handler.clone();
        tokio::spawn(async move { h.sync_task().await });

        handler
    }

    async fn sync_task(self: Arc<Self>) {
        let mut fast = interval(Duration::from_millis(300));
        let mut slow = interval(Duration::from_secs(self.inner.cfg.node_probe_interval_sec));

        loop {
            let self_clone = self.clone();
            let cancel = self_clone.inner.cancel_token.clone();
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("Handler sync cancelled");
                    break;
                }

                _ = fast.tick() => {
                    let any_leader_cons = self_clone.inner.leader_conns.read().await.iter().any(|c| match c {
                        Some(con) => !con.is_closed(),
                        None => false,
                    });

                    let any_follower_cons = self.inner.followers.read().await.values().any(|x| !x.is_closed());

                    if !any_leader_cons || !any_follower_cons{
                        self_clone.sync_with_cluster().await;
                    }
                }

                _ = slow.tick() => {
                    self_clone.sync_with_cluster().await;
                }
            }
        }
    }

    async fn sync_with_cluster(self: Arc<Self>) {
        match get_cluster_info(&self.inner.cfg, self.inner.discovery_map.clone()).await {
            Ok((leader_addr, followers)) => {
                self.clone().sync_leader(leader_addr).await;
                self.sync_followers(followers).await;
            }
            Err(e) => {
                tracing::debug!("failed to get cluster info: {:?}", e);
            }
        }
    }

    async fn sync_leader(self: Arc<Self>, leader_addr: String) {
        let required_vec: Vec<usize> = self
            .inner
            .leader_conns
            .read()
            .await
            .iter()
            .filter(|&x| match x {
                Some(c) => c.is_closed(),
                None => true,
            })
            .enumerate()
            .map(|x| x.0)
            .collect();
        for (_, i) in required_vec.iter().enumerate() {
            let leader_addr_clone = leader_addr.clone();
            let demux = DemuxMap::new();
            match Connection::connect(leader_addr_clone.clone(), &self.inner.cfg, demux).await {
                Ok(conn) => {
                    let stored_leader_addr = self.inner.leader_addr.load();
                    if let Some(stored) = stored_leader_addr.as_ref() {
                        if stored != &leader_addr_clone {
                            let _ = self
                                .inner
                                .metrics_tx
                                .try_send(MetricsEvent::BackendIncleaderChange);
                        }
                    };

                    // Store new connection
                    self.inner.leader_conns.write().await[*i] = Some(conn);
                    self.inner
                        .leader_addr
                        .store(Arc::new(Some(leader_addr_clone)));
                    let _ = self
                        .inner
                        .metrics_tx
                        .try_send(MetricsEvent::BackendOpenedConnections);
                }
                Err(e) => {
                    tracing::debug!("{:?}", e);
                }
            }
        }
    }

    async fn sync_followers(&self, followers: Vec<String>) {
        {
            let mut follower_nodes = self.inner.followers.write().await;
            let before = follower_nodes.len();
            // remove unavailable nodes
            follower_nodes.retain(|(id, _, _), _| followers.contains(id));
            let removed = before.saturating_sub(follower_nodes.len());
            if removed > 0 {
                for _ in 0..removed {
                    let _ = self
                        .inner
                        .metrics_tx
                        .try_send(MetricsEvent::BackendIncDisconnects);
                }
            }
        }

        // set metric
        let _ = self
            .inner
            .metrics_tx
            .try_send(MetricsEvent::BackendSetFollowers(followers.len() as u32));

        let mut required_conns: HashMap<String, u32> = HashMap::new(); // node, required connections
        let cur_node_ids: Vec<(String, u32)> = self
            .inner
            .followers
            .read()
            .await
            .keys()
            .map(|(name, idx, _)| (name.clone(), idx.clone()))
            .collect();

        let conn_per_roomzin_node = self.inner.cfg.conn_per_roomzin_node;
        for name in &followers {
            if !cur_node_ids.iter().any(|(x, _)| x == name) {
                required_conns.insert(name.clone(), conn_per_roomzin_node as u32);
            }
        }
        for (i, _) in cur_node_ids.iter() {
            let required =
                conn_per_roomzin_node - cur_node_ids.iter().filter(|(x, _)| x == i).count();
            if required > 0 {
                required_conns.insert(i.clone(), required as u32);
            }
        }
        let mut new_conns: HashMap<String, Vec<Option<Connection>>> = HashMap::new();
        for (name, &count) in required_conns.iter() {
            for _i in 0..count {
                match Connection::connect(name.clone(), &self.inner.cfg, DemuxMap::new()).await {
                    Ok(conn) => {
                        new_conns
                            .entry(name.clone())
                            .or_insert_with(|| vec![])
                            .push(Some(conn));
                        let _ = self
                            .inner
                            .metrics_tx
                            .try_send(MetricsEvent::BackendOpenedConnections);
                    }
                    Err(e) => {
                        tracing::debug!("connection error: {}", e);
                    }
                }
            }
        }

        let mut follower_nodes = self.inner.followers.write().await;
        let cur_node_ids: Vec<u32> = follower_nodes
            .iter()
            .map(|((_, idx, _), _)| idx)
            .cloned()
            .collect();
        let mut free_node_ids: Vec<u32> = vec![];
        for i in 0..followers.len() as u32 {
            if !cur_node_ids.contains(&i) && !free_node_ids.contains(&i) {
                free_node_ids.push(i);
            }
        }
        let mut new_assigned_node_ids: HashMap<String, u32> = HashMap::new();
        for key in self
            .inner
            .load_balancer
            .lock()
            .await
            .follower_history
            .keys()
        {
            new_assigned_node_ids.insert(key.0.clone(), key.1);
        }

        for (name, mut conns) in new_conns.into_iter() {
            let cur_conn_ids: Vec<u32> = follower_nodes
                .iter()
                .filter(|((n, _, _), c)| name == *n && !c.is_closed())
                .map(|((_, _, idx), _)| idx)
                .cloned()
                .collect();
            let mut free_conn_ids: Vec<u32> = vec![];
            for i in 0..self.inner.cfg.conn_per_roomzin_node as u32 {
                if !cur_conn_ids.contains(&i) {
                    free_conn_ids.push(i);
                }
            }

            let mut nidx = follower_nodes
                .iter()
                .find(|((n, _, _), _)| name == *n)
                .map(|((_, nidx, _), _)| nidx)
                .cloned()
                .unwrap_or(0); // take from indexes not in cur_node_ids
            if nidx == 0 {
                match new_assigned_node_ids.get(&name) {
                    Some(idx) => {
                        nidx = *idx;
                    }
                    None => {
                        if free_node_ids.len() > 0 {
                            let idx = free_node_ids.pop().unwrap_or_default();
                            new_assigned_node_ids.insert(name.clone(), idx);
                            nidx = idx;
                        } else {
                            continue;
                        }
                    }
                }
            }

            for i in 0..conns.len() {
                let c = match conns[i].take() {
                    Some(cn) => cn,
                    None => {
                        continue;
                    }
                };

                follower_nodes.insert((name.clone(), nidx, free_conn_ids[i]), c);
                self.inner
                    .load_balancer
                    .lock()
                    .await
                    .follower_history
                    .entry((name.clone(), nidx))
                    .or_insert(free_conn_ids[i]);
            }
        }
    }

    async fn next_follower_conn(&self) -> Option<Connection> {
        let len = self.inner.load_balancer.lock().await.follower_history.len();

        for _i in 1..=len {
            let mut lb = self.inner.load_balancer.lock().await;
            let mut next_node_indx = (lb.last_node_id + 1) as usize;
            if next_node_indx >= lb.follower_history.len() {
                next_node_indx = 0;
                lb.last_node_id = 0;
            }
            lb.last_node_id = next_node_indx as u32;

            let target = lb
                .follower_history
                .iter()
                .find(|&((_, nidx), _)| *nidx == next_node_indx as u32);
            if target.is_none() {
                continue;
            }

            let target = target.unwrap();
            let mut next_conn_id = *target.1;
            if next_conn_id >= self.inner.cfg.conn_per_roomzin_node as u32 {
                next_conn_id = 0;
            }
            let mut tried = 0;
            let mut followers = self.inner.followers.write().await;

            while tried < self.inner.cfg.conn_per_roomzin_node {
                let key: (String, u32, u32) = (target.0.0.clone(), target.0.1, next_conn_id);
                tried += 1;
                let mut closed = false;
                if let Some(c) = followers.get(&key) {
                    if c.is_closed() {
                        c.inner
                            .demux
                            .cleanup(Duration::from_secs(self.inner.cfg.timeout_sec * 2))
                            .await;
                        closed = true;
                    } else {
                        lb.last_node_id = target.0.1;
                        lb.follower_history
                            .entry((key.0, key.1))
                            .and_modify(|counter| {
                                *counter =
                                    (next_conn_id + 1) % self.inner.cfg.conn_per_roomzin_node as u32
                            })
                            .or_insert(1);
                        return Some(c.clone());
                    }
                }
                if closed == true {
                    followers.remove(&key);
                }
                next_conn_id += 1;
                if next_conn_id >= self.inner.cfg.conn_per_roomzin_node as u32 {
                    next_conn_id = 0;
                }
            }
        }
        None
    }

    async fn next_leader_conn(&self) -> Option<Connection> {
        let mut lb = self.inner.load_balancer.lock().await;
        let mut next = (lb.last_leader_id + 1) as usize;
        if next >= self.inner.cfg.conn_per_roomzin_node {
            next = 0;
        }
        let ld_vec = self.inner.leader_conns.write().await;
        if ld_vec.is_empty() || !ld_vec.iter().any(|x| x.is_some()) {
            return None;
        }
        for _i in 0..self.inner.cfg.conn_per_roomzin_node.min(ld_vec.len()) {
            let c = ld_vec.get(next).and_then(|x| x.as_ref());
            if c.is_none() {
                next += 1;
                if next >= self.inner.cfg.conn_per_roomzin_node {
                    next = 0;
                }
                continue;
            }
            let c = c.unwrap();
            if c.is_closed() {
                c.inner
                    .demux
                    .cleanup(Duration::from_secs(self.inner.cfg.timeout_sec * 2))
                    .await;
                let _ = ld_vec.get(next).take();
                next += 1;
                if next >= self.inner.cfg.conn_per_roomzin_node {
                    next = 0;
                }
                continue;
            }
            lb.last_leader_id = next as u32;
            return Some(c.clone());
        }

        None
    }

    /// Execute a request — writes go to a leader, reads go to next follower
    pub async fn execute(&self, is_write: bool, payload: Vec<u8>) -> Result<Bytes, RZError> {
        if payload.is_empty() {
            return Err(RZError::Validation("empty payload".into()));
        }

        let mut attempts = 0;
        loop {
            let conn = if is_write {
                // Early exit for writes when no leader is known
                match self.next_leader_conn().await {
                    Some(c) if !c.is_closed() => c.clone(),
                    _ => {
                        attempts += 1;
                        if attempts >= 3 {
                            return Err(RZError::NoLeaderAvailable);
                        }
                        sleep(Duration::from_millis(50 * (attempts as u64 + 1))).await;
                        continue;
                    }
                }
            } else {
                // Find best follower
                match self.next_follower_conn().await {
                    Some(c) if !c.is_closed() => c,
                    _ => {
                        attempts += 1;
                        if attempts >= 3 {
                            return Err(RZError::NoFollowerNodeAvailable);
                        }
                        sleep(Duration::from_millis(50 * (attempts as u64 + 1))).await;
                        continue;
                    }
                }
            };

            let (resp_tx, resp_rx) = oneshot::channel();
            let corr_id = conn.next_corr_id();
            let now = Instant::now();

            // Register the oneshot in demux before sending
            conn.inner.demux.store(corr_id, resp_tx, now).await;

            // Fire-and-forget the send — failure here means connection died mid-flight,
            // which will be caught by timeout/cleanup and trigger retry
            let _ = conn.send(corr_id, payload.clone()).await;

            // Wait for response
            // todo: wait with timeout
            match resp_rx.await {
                Ok(response_bytes) => return Ok(response_bytes),
                Err(_) => {
                    // oneshot was dropped → request timed out or connection closed
                    attempts += 1;
                    if attempts >= 3 {
                        return Err(RZError::Timeout);
                    }
                    sleep(Duration::from_millis(50 * (attempts as u64 + 1))).await;
                }
            }
        }
    }

    async fn external_discovery_task(self: Arc<Self>) {
        let refresh_interval = if self.inner.cfg.discovery_refresh_interval_sec == 0 {
            2 // default
        } else {
            self.inner.cfg.discovery_refresh_interval_sec
        };

        let mut interval = tokio::time::interval(Duration::from_secs(refresh_interval));
        let cancel_token = self.inner.cancel_token.clone();
        let discovery_addr = match &self.inner.cfg.discovery_addr {
            Some(addr) => addr.clone(),
            None => {
                tracing::error!("HTTP discovery enabled but discovery_addr not set");
                return;
            }
        };

        tracing::info!(
            "Starting HTTP discovery updater (interval: {}s)",
            refresh_interval
        );

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    tracing::info!("Discovery updater cancelled");
                    break;
                }
                _ = interval.tick() => {
                    match Self::call_external_discovery(&discovery_addr).await {
                        Ok(nodes) => {
                            let mut map = HashMap::new();
                            for node in nodes {
                                map.insert(node.node_id, (node.addr, Some(self.inner.cfg.roomzin_api_port as u32)));
                            }
                            tracing::debug!("Updated discovery map with {} nodes", map.len());
                            self.inner.discovery_map.store(Arc::new(map));
                        }
                        Err(e) => {
                            tracing::error!("Failed to fetch discovery nodes: {}", e);
                            // Keep existing map - don't clear it
                        }
                    }
                }
            }
        }
    }

    async fn call_external_discovery(addr: &str) -> Result<Vec<NodeAddr>, RZError> {
        let client = Client::builder().timeout(Duration::from_secs(5)).build()?;

        let response = client
            .get(addr)
            .send()
            .await
            .map_err(|e| RZError::Http(format!("Failed to fetch discovery: {}", e)))?;

        if !response.status().is_success() {
            return Err(RZError::Http(format!(
                "Discovery endpoint returned {}",
                response.status()
            )));
        }

        let discovery: DiscoveryYaml = response
            .json()
            .await
            .map_err(|e| RZError::Http(format!("Failed to parse discovery response: {}", e)))?;

        if discovery.nodes.is_empty() {
            tracing::warn!("Discovery endpoint returned empty node list");
        }

        Ok(discovery.nodes)
    }
}
