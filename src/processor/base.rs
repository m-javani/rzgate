// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use axum::response::Response;
use tokio::sync::mpsc::Sender;

use crate::{
    auth::AccessLevel,
    handler::handler::Handler,
    metrics::MetricsEvent,
    processor::{
        dec_room_avl::process_dec_room_avl, del_prop::process_del_prop,
        del_prop_day::process_del_prop_day, del_prop_room::process_del_prop_room,
        del_room_day::process_del_room_day, del_segment::process_del_segment,
        get_prop_room_day::process_get_prop_room_day, get_segments::process_get_segments,
        inc_room_avl::process_inc_room_avl, prop_exist::process_prop_exist,
        prop_room_date_list::process_prop_room_date_list, prop_room_exist::process_prop_room_exist,
        prop_room_list::process_prop_room_list, search_avail::process_search_avail,
        search_prop::process_search_prop, set_prop::process_set_prop,
        set_room_avl::process_set_room_avl, set_room_pkg::process_set_room_pkg,
    },
};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use bytes::Bytes;
use serde_json::{Value, from_slice};

pub const STATUS_KEY: &[u8] = br#""status":"#;
pub const MESSAGE_KEY: &[u8] = br#""message":"#;
pub const ERROR_VALUE: &[u8] = br#""error""#;
pub const SUCCESS_VALUE: &[u8] = br#""success""#;

pub async fn process(
    body: &[u8],
    handler: &Handler,
    metrics_tx: Sender<MetricsEvent>,
    access_level: AccessLevel,
) -> Response {
    let _ = metrics_tx.try_send(MetricsEvent::ApiIncCommands);
    let _ = metrics_tx.try_send(MetricsEvent::ApiAddBytesReceived(body.len() as u64));

    // Parse only once, minimal overhead for 150B
    let json: Value = match from_slice(body) {
        Ok(v) => v,
        Err(_) => return error_response(metrics_tx.clone(), "Invalid JSON").await,
    };

    // Get command with zero-copy reference
    let command = match json.get("command").and_then(|v| v.as_str()) {
        Some(cmd) => cmd,
        None => return error_response(metrics_tx.clone(), "Missing command field").await,
    };

    let is_write_command = matches!(
        command,
        "SETPROP"
            | "SETROOMPKG"
            | "SETROOMAVL"
            | "INCROOMAVL"
            | "DECROOMAVL"
            | "DELROOMDAY"
            | "DELPROPROOM"
            | "DELPROP"
            | "DELSEGMENT"
            | "DELPROPDAY"
    );

    if is_write_command && access_level == AccessLevel::ReadOnly {
        let _ = metrics_tx.try_send(MetricsEvent::ApiIncClientAuthFail); // optional new metric
        return error_response(metrics_tx, "Command not allowed with read-only token").await;
    }

    // Get body reference (doesn't copy data)
    let payload = json.get("body").unwrap_or(&Value::Null);

    match command.as_ref() {
        "SETPROP" => process_set_prop(payload, handler, metrics_tx.clone()).await,
        "PROPEXIST" => process_prop_exist(payload, handler, metrics_tx.clone()).await,
        "SEARCHPROP" => process_search_prop(payload, handler, metrics_tx.clone()).await,

        "SETROOMPKG" => process_set_room_pkg(payload, handler, metrics_tx.clone()).await,
        "SETROOMAVL" => process_set_room_avl(payload, handler, metrics_tx.clone()).await,
        "INCROOMAVL" => process_inc_room_avl(payload, handler, metrics_tx.clone()).await,
        "DECROOMAVL" => process_dec_room_avl(payload, handler, metrics_tx.clone()).await,
        "DELROOMDAY" => process_del_room_day(payload, handler, metrics_tx.clone()).await,
        "PROPROOMEXIST" => process_prop_room_exist(payload, handler, metrics_tx.clone()).await,
        "GETPROPROOMDAY" => process_get_prop_room_day(payload, handler, metrics_tx.clone()).await,
        "PROPROOMDATELIST" => {
            process_prop_room_date_list(payload, handler, metrics_tx.clone()).await
        }
        "DELPROPROOM" => process_del_prop_room(payload, handler, metrics_tx.clone()).await,

        "SEARCHAVAIL" => process_search_avail(payload, handler, metrics_tx.clone()).await,

        "PROPROOMLIST" => process_prop_room_list(payload, handler, metrics_tx.clone()).await,
        "DELPROP" => process_del_prop(payload, handler, metrics_tx.clone()).await,
        "DELSEGMENT" => process_del_segment(payload, handler, metrics_tx.clone()).await,
        "DELPROPDAY" => process_del_prop_day(payload, handler, metrics_tx.clone()).await,
        "GETSEGMENTS" => process_get_segments(payload, handler, metrics_tx.clone()).await,

        _ => error_response(metrics_tx.clone(), "unsupported command").await,
    }
}

pub async fn error_response(metrics_tx: Sender<MetricsEvent>, message: &str) -> Response {
    let _ = metrics_tx.try_send(MetricsEvent::ApiIncClientErrors);

    // We assume `message` is safe to embed (no user-controlled JSON escaping required)
    // If that ever changes, this function MUST be revisited.
    let msg_bytes = message.as_bytes();

    // {"status":"error","message":"..."}
    let mut buf = Vec::with_capacity(
        1 + STATUS_KEY.len() + ERROR_VALUE.len() + 1 + MESSAGE_KEY.len() + msg_bytes.len() + 2,
    );

    buf.push(b'{');

    // "status":"error"
    buf.extend_from_slice(STATUS_KEY);
    buf.extend_from_slice(ERROR_VALUE);
    buf.push(b',');

    // "message":"<message>"
    buf.extend_from_slice(MESSAGE_KEY);
    buf.push(b'"');
    buf.extend_from_slice(msg_bytes);
    buf.push(b'"');

    buf.push(b'}');

    (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, "application/json")],
        buf,
    )
        .into_response()
}

// change this to zero copy direct write to stream
pub fn prepend_header(clrid: u32, payload: &[u8]) -> Vec<u8> {
    let total_len = payload.len() as u32;
    let mut out = Vec::with_capacity(9 + payload.len());

    // Magic byte
    out.push(0xFF);

    // Client ID (4 bytes, little endian)
    out.extend_from_slice(&clrid.to_le_bytes());

    // Total length (4 bytes, little endian)
    out.extend_from_slice(&total_len.to_le_bytes());

    // Payload
    out.extend_from_slice(payload);

    out
}

// Field structure
#[derive(Debug, Clone)]
pub struct Field {
    pub id: u16,
    pub field_type: u8,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Codecs {
    pub rate_features: Vec<String>,
}

pub fn invalid_response() -> Response {
    (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, "application/json")],
        br#"{"status":"error","message":"INVALID_RESPONSE_FORMAT"}"#,
    )
        .into_response()
}

pub fn error_response_from_status(metrics_tx: Sender<MetricsEvent>, status: &[u8]) -> Response {
    let clean: Vec<u8> = status
        .iter()
        .copied()
        .filter(|b| *b >= 0x20 && *b <= 0x7E)
        .collect();

    let msg = if clean.is_empty() {
        b"UNKNOWN_ERROR".as_slice()
    } else {
        &clean
    };

    let mut json = Vec::new();
    json.extend_from_slice(br#"{"status":"error","message":""#);
    json.extend_from_slice(msg);
    json.extend_from_slice(br#""}"#);

    let _ = metrics_tx.try_send(MetricsEvent::ApiIncClientErrors);
    let _ = metrics_tx.try_send(MetricsEvent::ApiAddBytesSent(json.len() as u64));

    (
        StatusCode::BAD_REQUEST,
        [(header::CONTENT_TYPE, "application/json")],
        json,
    )
        .into_response()
}

/// Decodes a simple command response: expects either
/// - SUCCESS with 0 fields → success JSON
/// - ERROR with exactly one field (id=1, type=0x01 string) → error JSON with message
/// Returns appropriate Response, or invalid_response() on protocol errors
pub fn decode_simple_response(metrics_tx: Sender<MetricsEvent>, payload: &Bytes) -> Response {
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

    let offset = 1 + status_len + 2;

    if status != b"SUCCESS" {
        return handle_non_success_status(metrics_tx.clone(), data, status, field_count, offset);
    }

    // SUCCESS + 0 fields only
    if field_count == 0 {
        let rsp = br#"{"status":"success"}"#;
        let _ = metrics_tx.try_send(MetricsEvent::ApiAddBytesSent(rsp.len() as u64));
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            rsp,
        )
            .into_response();
    }

    // Any fields present = invalid for simple command
    invalid_response()
}

/// Decodes responses for commands that return a new u8 scalar value on success.
/// Expected formats:
/// - SUCCESS + 1 field (id=1, type=0x02, len=1) → {"status":"success","value":X}
/// - ERROR   + 1 field (id=1, type=0x01)        → {"status":"error","message":"..."}
///
/// The JSON field name for the value can be customized (e.g. "availability", "rate_feature_mask", etc.)
pub fn decode_scalar_u8_response(
    metrics_tx: Sender<MetricsEvent>,
    payload: &Bytes,
    value_field_name: &str,
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

    if status != b"SUCCESS" {
        return handle_non_success_status(metrics_tx.clone(), data, status, field_count, offset);
    }

    if field_count != 1 {
        return invalid_response();
    }

    if offset + 7 > data.len() {
        return invalid_response();
    }

    let field_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
    let field_type = data[offset + 2];

    if field_id != 1 || field_type != 0x02 {
        return invalid_response();
    }

    let field_len = u32::from_le_bytes([
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
    ]) as usize;
    offset += 7;

    if field_len != 1 || offset >= data.len() {
        return invalid_response();
    }

    let value = data[offset];

    let mut json = Vec::with_capacity(128);
    json.extend_from_slice(br#"{"status":"success",""#);
    json.extend_from_slice(value_field_name.as_bytes());
    json.extend_from_slice(br#"":"#);
    json.extend_from_slice(&value.to_string().into_bytes());
    json.push(b'}');

    let _ = metrics_tx.try_send(MetricsEvent::ApiAddBytesSent(json.len() as u64));

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json,
    )
        .into_response()
}

/// Decodes responses for commands that return a boolean (as u8: 1=true, 0=false) on success.
/// Expected formats:
/// - SUCCESS + 1 field (id=1, type=0x02, len=1, value=0 or 1) → {"status":"success","<field_name>":true/false}
/// - ERROR   + 1 field (id=1, type=0x01)                          → {"status":"error","message":"..."}
pub fn decode_boolean_response(
    metrics_tx: Sender<MetricsEvent>,
    payload: &Bytes,
    field_name: &str,
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

    if status != b"SUCCESS" {
        return handle_non_success_status(metrics_tx.clone(), data, status, field_count, offset);
    }

    if field_count != 1 {
        return invalid_response();
    }

    if offset + 7 > data.len() {
        return invalid_response();
    }

    let field_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
    let field_type = data[offset + 2];

    if field_id != 1 || field_type != 0x02 {
        return invalid_response();
    }

    let field_len = u32::from_le_bytes([
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
    ]) as usize;
    offset += 7;

    if field_len != 1 || offset >= data.len() {
        return invalid_response();
    }

    let value = data[offset];
    let boolean_slice: &[u8] = if value == 1 { b"true" } else { b"false" };

    let mut json = Vec::with_capacity(128);
    json.extend_from_slice(br#"{"status":"success",""#);
    json.extend_from_slice(field_name.as_bytes());
    json.extend_from_slice(br#"":"#);
    json.extend_from_slice(boolean_slice);
    json.push(b'}');

    let _ = metrics_tx.try_send(MetricsEvent::ApiAddBytesSent(json.len() as u64));

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json,
    )
        .into_response()
}

/// Handles any status other than "SUCCESS".
/// Tries to extract a meaningful error message from the first field (id=1, type=0x01),
/// otherwise falls back to generic status-based error.
pub fn handle_non_success_status(
    metrics_tx: Sender<MetricsEvent>,
    data: &[u8],
    status: &[u8],
    field_count: u16,
    mut offset: usize,
) -> Response {
    // Try to extract message from first field
    if field_count >= 1 && offset + 7 <= data.len() {
        let field_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let field_type = data[offset + 2];

        if field_id == 1 && field_type == 0x01 {
            let field_len = u32::from_le_bytes([
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
            ]) as usize;
            offset += 7;

            if offset + field_len <= data.len() {
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

                let _ = metrics_tx.try_send(MetricsEvent::ApiIncClientErrors);
                let _ = metrics_tx.try_send(MetricsEvent::ApiAddBytesSent(json.len() as u64));

                return (
                    StatusCode::BAD_REQUEST,
                    [(header::CONTENT_TYPE, "application/json")],
                    json,
                )
                    .into_response();
            }
        }
    }

    // Fallback
    error_response_from_status(metrics_tx.clone(), status)
}
