// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RZError {
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("roomzin unreachable: {0}")]
    RoomzinUnreachable(String),
    #[error("Request timeoutw")]
    Timeout,
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("network error: {0}")]
    Network(String),
}
