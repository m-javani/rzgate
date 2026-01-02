// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::helper::{bitmask_to_rate_feature_strings, bytes_to_property_id};
use crate::metrics::MetricsEvent;
use crate::processor::base::{handle_non_success_status, invalid_response};
use crate::{handler::handler::Handler, processor::base::error_response};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc::Sender;

pub async fn process_get_prop_room_day(
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

    // Build binary payload
    let mut buf = Vec::new();

    let cmd_name = "GETPROPROOMDAY";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());

    // Field count: always 3
    buf.extend_from_slice(&3u16.to_le_bytes());

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

    // Field 3: date
    buf.extend_from_slice(&0x03u16.to_le_bytes());
    buf.push(0x01);
    buf.extend_from_slice(&(date.len() as u32).to_le_bytes());
    buf.extend_from_slice(date.as_bytes());

    match handler.execute(false, buf).await {
        Ok(field_data) => decode_get_prop_room_day_response(metrics_tx.clone(), &field_data),
        Err(e) => error_response(metrics_tx.clone(), &e.to_string()).await,
    }
}

fn decode_get_prop_room_day_response(
    metrics_tx: Sender<MetricsEvent>,
    payload: &Bytes,
) -> Response {
    let data = payload.as_ref();

    if data.is_empty() {
        return invalid_response();
    }

    let status_len = data[0] as usize;
    let min_len = 1 + status_len + 2;
    if data.len() < min_len {
        return invalid_response();
    }

    let status = &data[1..1 + status_len];
    let field_count = u16::from_le_bytes([data[1 + status_len], data[1 + status_len + 1]]);

    let mut offset = 1 + status_len + 2;

    // Use shared helper for any non-SUCCESS status
    if status != b"SUCCESS" {
        return handle_non_success_status(metrics_tx.clone(), data, status, field_count, offset);
    }

    // Must have exactly 5 fields
    if field_count != 5 {
        return invalid_response();
    }

    // Helper to read a field safely
    macro_rules! read_field {
        () => {{
            if offset + 7 > data.len() {
                return invalid_response();
            }
            let id = u16::from_le_bytes([data[offset], data[offset + 1]]);
            let typ = data[offset + 2];
            let len = u32::from_le_bytes([
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
            ]) as usize;
            offset += 7;
            if offset + len > data.len() {
                return invalid_response();
            }
            let slice = &data[offset..offset + len];
            offset += len;
            (id, typ, slice)
        }};
    }

    // Field 1: property_id (string)
    let (id1, typ1, property_id_bytes) = read_field!();
    if id1 != 1 || typ1 != 0x01 {
        return invalid_response();
    }

    // Field 2: date (string)
    let (id2, typ2, date_bytes) = read_field!();
    if id2 != 2 || typ2 != 0x01 {
        return invalid_response();
    }
    let date_str = std::str::from_utf8(date_bytes).unwrap_or("invalid-date");

    // Field 3: availability (u8)
    let (id3, typ3, avail_bytes) = read_field!();
    if id3 != 3 || typ3 != 0x02 || avail_bytes.len() != 1 {
        return invalid_response();
    }
    let availability = avail_bytes[0];

    // Field 4: final_price (u32)
    let (id4, typ4, price_bytes) = read_field!();
    if id4 != 4 || typ4 != 0x03 || price_bytes.len() != 4 {
        return invalid_response();
    }
    let final_price = u32::from_le_bytes([
        price_bytes[0],
        price_bytes[1],
        price_bytes[2],
        price_bytes[3],
    ]);

    // Field 5: rate_feature_mask (u32)
    let (id5, typ5, rate_bytes) = read_field!();
    if id5 != 5 || typ5 != 0x03 || rate_bytes.len() != 4 {
        return invalid_response();
    }
    let rate_feature_mask =
        u32::from_le_bytes([rate_bytes[0], rate_bytes[1], rate_bytes[2], rate_bytes[3]]);

    // Must consume entire payload
    if offset != data.len() {
        return invalid_response();
    }

    // Convert rate mask to feature strings
    let rate_features = match bitmask_to_rate_feature_strings(rate_feature_mask) {
        Ok(features) => features,
        Err(_) => return invalid_response(),
    };

    // Build JSON response efficiently
    let mut json = Vec::with_capacity(512);
    json.extend_from_slice(br#"{"status":"success","property_id":""#);
    json.extend_from_slice(bytes_to_property_id(property_id_bytes).as_bytes());
    json.extend_from_slice(br#"","date":""#);
    json.extend_from_slice(date_str.as_bytes());
    json.extend_from_slice(br#"","availability":"#);
    json.extend_from_slice(&availability.to_string().into_bytes());
    json.extend_from_slice(br#","final_price":"#);
    json.extend_from_slice(&final_price.to_string().into_bytes());
    json.extend_from_slice(br#","rate_feature":["#);

    for (i, feature) in rate_features.iter().enumerate() {
        if i > 0 {
            json.push(b',');
        }
        json.push(b'"');
        json.extend_from_slice(feature.as_bytes());
        json.push(b'"');
    }

    json.extend_from_slice(br#"]}"#);

    let _ = metrics_tx.try_send(MetricsEvent::ApiAddBytesSent(json.len() as u64));

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json,
    )
        .into_response()
}
