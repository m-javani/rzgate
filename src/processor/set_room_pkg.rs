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

pub async fn process_set_room_pkg(
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

    // Optional fields
    let availability = payload
        .get("availability")
        .and_then(|v| v.as_u64())
        .map(|v| {
            if v > 255 {
                None // out of range for u8
            } else {
                Some(v as u8)
            }
        })
        .flatten();

    let final_price = payload
        .get("final_price")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32); // assuming price fits in u32

    let rate_feature = match payload.get("rate_features").and_then(|v| v.as_array()) {
        Some(arr) => {
            let features: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            if features.is_empty() {
                None
            } else {
                Some(features.join(","))
            }
        }
        None => None,
    };

    // Build binary payload
    let mut buf = Vec::new();

    // Command name
    let cmd_name = "SETROOMPKG";
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

    // Required fields
    add_field(0x01, 0x01, property_id.as_bytes());
    add_field(0x02, 0x01, room_type.as_bytes());
    add_field(0x03, 0x01, date.as_bytes());

    // Optional fields
    if let Some(avail) = availability {
        add_field(0x04, 0x02, &[avail]);
    }
    if let Some(price) = final_price {
        add_field(0x05, 0x03, &price.to_le_bytes());
    }
    if let Some(features) = rate_feature {
        add_field(0x06, 0x01, features.as_bytes());
    }

    // Patch field count
    let fc_bytes = field_count.to_le_bytes();
    buf[field_count_pos] = fc_bytes[0];
    buf[field_count_pos + 1] = fc_bytes[1];

    match handler.execute(true, buf).await {
        Ok(field_data) => decode_set_room_pkg_response(metrics_tx.clone(), &field_data),
        Err(e) => error_response(metrics_tx.clone(), &e.to_string()).await,
    }
}

fn decode_set_room_pkg_response(metrics_tx: Sender<MetricsEvent>, payload: &Bytes) -> Response {
    decode_simple_response(metrics_tx, payload)
}
