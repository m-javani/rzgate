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

pub async fn process_get_segments(
    payload: &Value,
    handler: &Handler,
    metrics_tx: Sender<MetricsEvent>,
) -> Response {
    // No fields expected in request — but we still validate it's an object (even empty)
    if !payload.is_object() {
        return error_response(metrics_tx.clone(), "invalid payload").await;
    }

    // Build binary payload — no fields
    let mut buf = Vec::new();

    let cmd_name = "GETSEGMENTS";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());

    // Field count = 0
    buf.extend_from_slice(&0u16.to_le_bytes());

    match handler.execute(false, buf).await {
        Ok(field_data) => decode_get_segments_response(metrics_tx.clone(), &field_data),
        Err(e) => error_response(metrics_tx.clone(), &e.to_string()).await,
    }
}
fn decode_get_segments_response(metrics_tx: Sender<MetricsEvent>, payload: &Bytes) -> Response {
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

    // --- SUCCESS: empty result ---
    if field_count == 0 {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            br#"{"status":"success","segments":[]}"#,
        )
            .into_response();
    }

    // Must have even number of fields (complete name/count pairs)
    if field_count % 2 != 0 {
        return invalid_response();
    }

    let mut segments = Vec::with_capacity((field_count / 2) as usize);
    let mut expected_id: u16 = 1;

    while offset + 7 <= data.len() {
        let field_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        if field_id != expected_id {
            return invalid_response();
        }

        let field_type = data[offset + 2];
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

        let field_data = &data[offset..offset + field_len];
        offset += field_len;

        if expected_id % 2 == 1 {
            // Odd: segment name → must be string
            if field_type != 0x01 {
                return invalid_response();
            }
            // Start new segment entry
            segments.push((field_data.to_vec(), None));
        } else {
            // Even: propCount → must be u32 (type 0x03, len=4)
            if field_type != 0x03 || field_len != 4 {
                return invalid_response();
            }
            let prop_count =
                u32::from_le_bytes([field_data[0], field_data[1], field_data[2], field_data[3]]);

            if let Some(last) = segments.last_mut() {
                last.1 = Some(prop_count);
            } else {
                return invalid_response(); // should never happen
            }
        }

        expected_id += 1;
    }

    // Must have consumed all fields
    if (expected_id - 1) != field_count {
        return invalid_response();
    }

    // Build JSON response
    let mut json = Vec::with_capacity(
        512 + segments
            .iter()
            .map(|(name, count)| name.len() + count.map(|c| c.to_string().len()).unwrap_or(0) + 32)
            .sum::<usize>(),
    );

    json.extend_from_slice(br#"{"status":"success","segments":["#);

    for (i, (segment_name, prop_count)) in segments.iter().enumerate() {
        if i > 0 {
            json.push(b',');
        }
        json.push(b'{');
        json.extend_from_slice(br#""segment":""#);
        json.extend_from_slice(segment_name);
        json.extend_from_slice(br#"","propCount":"#);
        json.extend_from_slice(&prop_count.unwrap().to_string().into_bytes());
        json.push(b'}');
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
