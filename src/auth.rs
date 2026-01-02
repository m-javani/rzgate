// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use arc_swap::ArcSwap;
use rustc_hash::FxHashSet;
use std::{sync::Arc, time::Duration};
use tokio::{fs, time};
use tokio_util::sync::CancellationToken;

/// Static tokens using ArcSwap for lock-free reads and reloads
static TOKENS: tokio::sync::OnceCell<ArcSwap<AuthTokens>> = tokio::sync::OnceCell::const_new();

#[derive(Debug, serde::Deserialize)]
struct AuthFile {
    roomzin_token: String,
    #[serde(default)]
    full_access_tokens: Vec<String>,
    #[serde(default)]
    read_only_tokens: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AuthTokens {
    pub roomzin_token: String,
    pub full_access_tokens: FxHashSet<String>,
    pub read_only_tokens: FxHashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessLevel {
    Full,
    ReadOnly,
}

/// Initialize tokens from auth file (called once at startup)
pub async fn init_tokens(tokens_path: &str) -> Result<(), String> {
    let tokens = load_tokens(tokens_path).await?;
    let auth_tokens = build_auth_tokens(tokens);
    TOKENS
        .set(ArcSwap::from_pointee(auth_tokens))
        .map_err(|_| "Tokens already loaded".to_string())?;
    tracing::info!("Auth tokens loaded from: {}", tokens_path);
    Ok(())
}

/// Load and parse the YAML file
async fn load_tokens(tokens: &str) -> Result<AuthFile, String> {
    let contents = fs::read_to_string(tokens)
        .await
        .map_err(|e| format!("Failed to read auth file: {}", e))?;

    let tokens: AuthFile =
        serde_yaml::from_str(&contents).map_err(|e| format!("Failed to parse auth file: {}", e))?;

    Ok(tokens)
}

/// Convert AuthFile into the in-memory structure with HashSets
fn build_auth_tokens(file: AuthFile) -> AuthTokens {
    let full_access_tokens = file
        .full_access_tokens
        .into_iter()
        .collect::<FxHashSet<_>>();
    let read_only_tokens = file.read_only_tokens.into_iter().collect::<FxHashSet<_>>();

    AuthTokens {
        roomzin_token: file.roomzin_token,
        full_access_tokens,
        read_only_tokens,
    }
}

/// Get Roomzin token (for connecting to the external backend server)
pub fn get_roomzin_token() -> String {
    let arc_swap = TOKENS.get().expect("Auth tokens not initialized");
    let tokens = arc_swap.load();
    tokens.roomzin_token.clone()
}

/// Returns the access level for a valid rzgate token.
/// Returns None if the token is not present in either list.
pub fn get_access_level(token: &str) -> Option<AccessLevel> {
    let arc_swap = TOKENS.get()?;
    let tokens = arc_swap.load();

    if tokens.full_access_tokens.contains(token) {
        Some(AccessLevel::Full)
    } else if tokens.read_only_tokens.contains(token) {
        Some(AccessLevel::ReadOnly)
    } else {
        None
    }
}

/// Legacy function - kept for backward compatibility if anything still uses it
/// (can be removed later if no longer needed)
pub fn validate_rzgate_token(token: &str) -> bool {
    get_access_level(token).is_some()
}

/// Watch auth file for changes and reload tokens automatically
pub async fn start_watcher(tokens: String, cancel_token: CancellationToken) {
    let mut interval = time::interval(Duration::from_secs(5));
    let mut last_modified = match fs::metadata(&tokens).await {
        Ok(metadata) => metadata
            .modified()
            .unwrap_or_else(|_| std::time::SystemTime::now()),
        Err(e) => {
            tracing::error!("Failed to get auth file metadata: {}", e);
            return;
        }
    };

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Ok(metadata) = fs::metadata(&tokens).await {
                    if let Ok(modified) = metadata.modified() {
                        if modified > last_modified {
                            match reload_tokens(&tokens).await {
                                Ok(_) => {
                                    last_modified = modified;
                                    tracing::info!("Auth tokens reloaded successfully");
                                }
                                Err(e) => tracing::error!("Failed to reload auth tokens: {}", e),
                            }
                        }
                    }
                }
            }
            _ = cancel_token.cancelled() => {
                tracing::info!("Auth watcher cancelled");
                break;
            }
        }
    }
}

/// Reload tokens from file and update the ArcSwap store
async fn reload_tokens(tokens: &str) -> Result<(), String> {
    let file_tokens = load_tokens(tokens).await?;
    let new_tokens = build_auth_tokens(file_tokens);

    let arc_swap = TOKENS.get().ok_or("Auth tokens not initialized")?;
    arc_swap.store(Arc::new(new_tokens));
    Ok(())
}

/// For testing/debugging: Get current token info
#[cfg(test)]
pub fn debug_tokens() -> Option<String> {
    TOKENS.get().map(|arc_swap| {
        let tokens = arc_swap.load();
        format!(
            "roomzin: {}, full_access_count: {}, read_only_count: {}",
            tokens.roomzin_token,
            tokens.full_access_tokens.len(),
            tokens.read_only_tokens.len()
        )
    })
}
