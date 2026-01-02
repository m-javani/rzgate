// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::metrics::MetricsEvent;
use crate::processor::base::{handle_non_success_status, invalid_response};
use crate::{handler::handler::Handler, processor::base::error_response};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc::Sender;

pub async fn process_prop_room_list(
    payload: &Value,
    handler: &Handler,
    metrics_tx: Sender<MetricsEvent>,
) -> Response {
    // Required field
    let property_id = match payload.get("property_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics_tx.clone(), "property_id is required").await,
    };

    // Build binary payload
    let mut buf = Vec::new();

    // Command name
    let cmd_name = "PROPROOMLIST";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());

    // Field count: always 1
    buf.extend_from_slice(&1u16.to_le_bytes());

    // Single field: ID=0x01, type=0x01, property_id string
    buf.extend_from_slice(&0x01u16.to_le_bytes());
    buf.push(0x01);
    buf.extend_from_slice(&(property_id.len() as u32).to_le_bytes());
    buf.extend_from_slice(property_id.as_bytes());

    match handler.execute(false, buf).await {
        Ok(field_data) => decode_prop_room_list_response(metrics_tx.clone(), &field_data),
        Err(e) => error_response(metrics_tx.clone(), &e.to_string()).await,
    }
}

fn decode_prop_room_list_response(metrics_tx: Sender<MetricsEvent>, payload: &Bytes) -> Response {
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

    // --- SUCCESS: parse list of room types ---
    if field_count == 0 {
        let rsp = br#"{"status":"success","room_types":[]}"#;
        let _ = metrics_tx.try_send(MetricsEvent::ApiAddBytesSent(rsp.len() as u64));

        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            rsp,
        )
            .into_response();
    }

    let mut room_types = Vec::with_capacity(field_count as usize);
    let mut expected_id: u16 = 1;

    while offset + 7 <= data.len() {
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

        let room_type_bytes = &data[offset..offset + field_len];
        // Simple UTF-8 conversion — matches Go's string(fields[i].Data)
        let room_type = String::from_utf8_lossy(room_type_bytes).to_string();

        room_types.push(room_type);
        offset += field_len;
        expected_id += 1;
    }

    // Must consume exactly all fields
    if (expected_id - 1) != field_count {
        return invalid_response();
    }

    // Build JSON response
    let mut json = Vec::with_capacity(256 + room_types.iter().map(|s| s.len() + 4).sum::<usize>());
    json.extend_from_slice(br#"{"status":"success","room_types":["#);

    for (i, room_type) in room_types.iter().enumerate() {
        if i > 0 {
            json.push(b',');
        }
        json.push(b'"');
        json.extend_from_slice(room_type.as_bytes());
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
