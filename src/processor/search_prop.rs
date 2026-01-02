// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::helper::bytes_to_property_id;
use crate::metrics::MetricsEvent;
use crate::processor::base::{handle_non_success_status, invalid_response};
use crate::{handler::handler::Handler, processor::base::error_response};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc::Sender;

pub async fn process_search_prop(
    payload: &Value,
    handler: &Handler,
    metrics_tx: Sender<MetricsEvent>,
) -> Response {
    // extract, validate and build binary payload from fields
    let segment = match payload.get("segment").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return error_response(metrics_tx.clone(), "segment is required").await,
    };
    let area = payload.get("area").and_then(|v| v.as_str());
    let property_type = payload.get("property_type").and_then(|v| v.as_str());
    let category = payload.get("category").and_then(|v| v.as_str());
    let stars = payload
        .get("stars")
        .and_then(|v| v.as_u64())
        .filter(|&s| s >= 1 && s <= 5)
        .map(|s| s as u8);
    let latitude = payload
        .get("latitude")
        .and_then(|v| v.as_f64())
        .filter(|&lat| lat >= -90.0 && lat <= 90.0);
    let longitude = payload
        .get("longitude")
        .and_then(|v| v.as_f64())
        .filter(|&lon| lon >= -180.0 && lon <= 180.0);
    let amenities = match payload.get("amenities").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => Vec::new(),
    };
    let limit = payload.get("limit").and_then(|v| v.as_u64());

    // Build payload inline
    let mut buf = Vec::new();
    // Command name
    let cmd_name = "SEARCHPROP";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());
    // Field count placeholder (we'll patch this later)
    let field_count_pos = buf.len();
    buf.extend_from_slice(&0u16.to_le_bytes()); // placeholder for field count
    // Helper to add a field
    let mut field_count = 0u16;
    let mut add_field = |id: u16, typ: u8, data: &[u8]| {
        buf.extend_from_slice(&id.to_le_bytes());
        buf.push(typ);
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(data);
        field_count += 1;
    };
    // Add fields (required + optional)
    add_field(0x01, 0x01, segment.as_bytes()); // Segment (required)
    if let Some(a) = area {
        add_field(0x02, 0x01, a.as_bytes()); // Area
    }
    if let Some(t) = property_type {
        add_field(0x03, 0x01, t.as_bytes()); // PropertyType
    }
    if let Some(s) = stars {
        add_field(0x04, 0x02, &[s]); // Stars
    }
    if let Some(c) = category {
        add_field(0x05, 0x01, c.as_bytes()); // Category
    }
    if !amenities.is_empty() {
        let amenity_str = amenities.join(",");
        add_field(0x06, 0x01, amenity_str.as_bytes()); // Amenities
    }
    if let Some(lon) = longitude {
        add_field(0x07, 0x03, &lon.to_le_bytes()); // Longitude
    }
    if let Some(lat) = latitude {
        add_field(0x08, 0x03, &lat.to_le_bytes()); // Latitude
    }
    if let Some(l) = limit {
        add_field(0x09, 0x03, &l.to_le_bytes()); // Limit
    }
    // Patch field count
    let field_count_bytes = field_count.to_le_bytes();
    buf[field_count_pos] = field_count_bytes[0];
    buf[field_count_pos + 1] = field_count_bytes[1];

    match handler.execute(false, buf).await {
        Ok(field_data) => decode_search_prop_response(metrics_tx.clone(), &field_data),
        Err(e) => error_response(metrics_tx.clone(), &e.to_string()).await,
    }
}

fn decode_search_prop_response(metrics_tx: Sender<MetricsEvent>, payload: &Bytes) -> Response {
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

    let offset_after_header = 1 + status_len + 2;
    if status != b"SUCCESS" {
        return handle_non_success_status(
            metrics_tx.clone(),
            data,
            status,
            field_count,
            offset_after_header,
        );
    }

    // --- SUCCESS status below this point ---
    // Empty result
    if field_count == 0 {
        let rsp = br#"{"status":"success","properties":[]}"#;
        let _ = metrics_tx.try_send(MetricsEvent::ApiAddBytesSent(rsp.len() as u64));
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            rsp,
        )
            .into_response();
    }

    // --- SUCCESS + exactly 1 field → treat as error (current server behavior) ---
    if field_count == 1 {
        if offset + 7 > data.len() {
            return invalid_response();
        }

        let field_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let field_type = data[offset + 2];

        if field_id != 1 || field_type != 0x01 {
            return invalid_response();
        }

        let field_len = u32::from_le_bytes([
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
        ]) as usize;
        offset += 7;

        if offset + field_len > data.len() {
            return invalid_response();
        }

        let message = &data[offset..offset + field_len];
        let msg = if message.is_empty() {
            b"UNKNOWN_ERROR"
        } else {
            message
        };

        let mut json = Vec::with_capacity(64 + msg.len());
        json.extend_from_slice(br#"{"status":"error","message":""#);
        json.extend_from_slice(msg);
        json.extend_from_slice(br#""}"#);

        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            json,
        )
            .into_response();
    }

    // --- SUCCESS + multiple fields → property list ---
    let mut properties = Vec::with_capacity(field_count as usize);

    for expected_id in 1..=field_count {
        if offset + 7 > data.len() {
            return invalid_response();
        }

        let field_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        if field_id != expected_id {
            return invalid_response();
        }

        let field_type = data[offset + 2];
        if field_type != 0x01 {
            return invalid_response();
        }

        let field_len = u32::from_le_bytes([
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
        ]) as usize;
        offset += 7;

        if offset + field_len > data.len() {
            return invalid_response();
        }

        let raw_bytes = &data[offset..offset + field_len];
        let prop_id = bytes_to_property_id(raw_bytes);

        properties.push(prop_id);
        offset += field_len;
    }

    // Build final JSON
    let mut json = Vec::with_capacity(256 + properties.iter().map(|s| s.len() + 4).sum::<usize>());
    json.extend_from_slice(br#"{"status":"success","properties":["#);

    for (i, prop) in properties.iter().enumerate() {
        if i > 0 {
            json.push(b',');
        }
        json.push(b'"');
        json.extend_from_slice(prop.as_bytes());
        json.push(b'"');
    }

    json.extend_from_slice(br#"]}"#);

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json,
    )
        .into_response()
}
