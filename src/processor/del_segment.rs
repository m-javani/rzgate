// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::metrics::MetricsEvent;
use crate::processor::base::decode_simple_response;
use crate::{handler::handler::Handler, processor::base::error_response};
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc::Sender;

pub async fn process_del_segment(
    payload: &Value,
    handler: &Handler,
    metrics_tx: Sender<MetricsEvent>,
) -> Response {
    // Required field
    let segment = match payload.get("segment").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics_tx.clone(), "segment is required").await,
    };

    // Build binary payload
    let mut buf = Vec::new();

    let cmd_name = "DELSEGMENT";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());

    // Field count: always 1
    buf.extend_from_slice(&1u16.to_le_bytes());

    // Field: ID=0x01, type=0x01, segment string
    buf.extend_from_slice(&0x01u16.to_le_bytes());
    buf.push(0x01);
    buf.extend_from_slice(&(segment.len() as u32).to_le_bytes());
    buf.extend_from_slice(segment.as_bytes());

    match handler.execute(true, buf).await {
        Ok(field_data) => decode_del_segment_response(metrics_tx.clone(), &field_data),
        Err(e) => error_response(metrics_tx.clone(), &e.to_string()).await,
    }
}

fn decode_del_segment_response(metrics_tx: Sender<MetricsEvent>, payload: &Bytes) -> Response {
    decode_simple_response(metrics_tx, payload)
}
