// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::error::RZError;
use crate::metrics::MetricsRef;
use crate::protocol::response::decode_simple_response;
use crate::{handler::handler::Handler, protocol::error_response};
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;

pub async fn process_del_room_day(
    seg: &str,
    payload: &Value,
    handler: &Handler,
    metrics: MetricsRef,
) -> Response {
    // Required fields
    let property_id = match payload.get("property_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return error_response(
                metrics,
                RZError::Validation("property_id is required".into()),
            )
            .await;
        }
    };
    let room_type = match payload.get("room_type").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return error_response(metrics, RZError::Validation("room_type is required".into()))
                .await;
        }
    };
    let date = match payload.get("date").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics, RZError::Validation("date is required".into())).await,
    };

    // Build binary payload
    let mut buf = Vec::new();

    let cmd_name = "DELROOMDAY";
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

    match handler.execute(seg, true, buf).await {
        Ok(field_data) => decode_del_room_day_response(metrics, &field_data),
        Err(e) => error_response(metrics, e).await,
    }
}

fn decode_del_room_day_response(metrics: MetricsRef, payload: &Bytes) -> Response {
    decode_simple_response(metrics, payload)
}
