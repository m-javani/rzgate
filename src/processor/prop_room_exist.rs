// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::metrics::MetricsRef;
use crate::protocol::response::decode_boolean_response;
use crate::{handler::handler::Handler, protocol::error_response};
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;

pub async fn process_prop_room_exist(
    seg: &str,
    payload: &Value,
    handler: &Handler,
    metrics: MetricsRef,
) -> Response {
    // Required fields
    let property_id = match payload.get("property_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics, "property_id is required").await,
    };
    let room_type = match payload.get("room_type").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics, "room_type is required").await,
    };

    // Build binary payload
    let mut buf = Vec::new();

    // Command name
    let cmd_name = "PROPROOMEXIST";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());

    // Field count: always 2
    buf.extend_from_slice(&2u16.to_le_bytes());

    // Field 1: property_id
    buf.extend_from_slice(&0x01u16.to_le_bytes());
    buf.push(0x01);
    buf.extend_from_slice(&(property_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(property_id.as_bytes());

    // Field 2: room_type
    buf.extend_from_slice(&0x02u16.to_le_bytes());
    buf.push(0x01);
    buf.extend_from_slice(&(room_type.len() as u32).to_le_bytes());
    buf.extend_from_slice(room_type.as_bytes());

    match handler.execute(seg, false, buf).await {
        Ok(field_data) => decode_prop_room_exist_response(metrics, &field_data),
        Err(e) => error_response(metrics, &e.to_string()).await,
    }
}

fn decode_prop_room_exist_response(metrics: MetricsRef, payload: &Bytes) -> Response {
    decode_boolean_response(metrics, payload, "exists")
}
