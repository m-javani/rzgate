// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::metrics::MetricsEvent;
use crate::processor::base::decode_scalar_u8_response;
use crate::{handler::handler::Handler, processor::base::error_response};
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc::Sender;

pub async fn process_inc_room_avl(
    payload: &Value,
    handler: &Handler,
    metrics_tx: Sender<MetricsEvent>,
) -> Response {
    // Required fields
    let property_id = match payload.get("property_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics_tx.clone(), "property_id is required").await,
    };
    let room_type = match payload.get("room_type").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics_tx.clone(), "room_type is required").await,
    };
    let date = match payload.get("date").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics_tx.clone(), "date is required").await,
    };
    let amount = match payload.get("amount").and_then(|v| v.as_u64()) {
        Some(a) if a <= 255 => a as u8,
        _ => {
            return error_response(metrics_tx.clone(), "amount is required and must be 0-255")
                .await;
        }
    };

    // Build binary payload
    let mut buf = Vec::new();

    // Command name
    let cmd_name = "INCROOMAVL";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());

    // Field count placeholder
    let field_count_pos = buf.len();
    buf.extend_from_slice(&0u16.to_le_bytes());

    // Helper to add field
    let mut field_count = 0u16;
    let mut add_field = |id: u16, typ: u8, data: &[u8]| {
        buf.extend_from_slice(&id.to_le_bytes());
        buf.push(typ);
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(data);
        field_count += 1;
    };

    // All 4 fields are required
    add_field(0x01, 0x01, property_id.as_bytes());
    add_field(0x02, 0x01, room_type.as_bytes());
    add_field(0x03, 0x01, date.as_bytes());
    add_field(0x04, 0x02, &[amount]);

    // Patch field count (always 4)
    buf[field_count_pos] = 4u8;
    buf[field_count_pos + 1] = 0u8;

    match handler.execute(true, buf).await {
        Ok(field_data) => decode_inc_room_avl_response(metrics_tx.clone(), &field_data),
        Err(e) => error_response(metrics_tx.clone(), &e.to_string()).await,
    }
}

fn decode_inc_room_avl_response(metrics_tx: Sender<MetricsEvent>, payload: &Bytes) -> Response {
    decode_scalar_u8_response(metrics_tx, payload, "availability")
}
