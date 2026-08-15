use crate::metrics::MetricsEvent;
use crate::protocol::invalid_response;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::response::Response;
use tokio::sync::mpsc::Sender;

use bytes::Bytes;

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

    // Fallback: sanitize status bytes and build error response
    // Fallback: sanitize status bytes and build error response
    let mut clean: Vec<u8> = status
        .iter()
        .copied()
        .filter(|b| *b >= 0x20 && *b <= 0x7E)
        .collect();

    if clean.is_empty() {
        clean.extend_from_slice(b"UNKNOWN_ERROR");
    }

    let msg = &clean;

    let mut json = Vec::with_capacity(64 + msg.len());
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
