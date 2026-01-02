// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use once_cell::sync::OnceCell;

use crate::{error::RZError, processor::base::Codecs};

pub static RATE_FEATURES: OnceCell<Vec<&'static str>> = OnceCell::new();

pub fn get_rate_features() -> Result<&'static Vec<&'static str>, RZError> {
    RATE_FEATURES
        .get()
        .ok_or_else(|| RZError::Validation("RATE_FEATURES not initialized".to_string()))
}

pub fn get_codecs() -> Codecs {
    let rate_features = RATE_FEATURES
        .get()
        .map_or(Vec::new(), |v| v.iter().map(|&s| s.to_string()).collect());

    Codecs { rate_features }
}

pub fn set_codecs(codecs: Codecs) -> Result<(), RZError> {
    // Convert to static strings and store in OnceCell
    let rate_features: Vec<&'static str> = codecs
        .rate_features
        .into_iter()
        .map(|s| Box::leak(s.into_boxed_str()) as &'static str)
        .collect();

    // Initialize OnceCell values
    RATE_FEATURES
        .set(rate_features)
        .map_err(|_| RZError::Validation("Failed to set RATE_FEATURES".to_string()))?;

    Ok(())
}
