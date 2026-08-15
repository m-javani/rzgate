use crate::error::RZError;
use crate::metrics::MetricsRef;
use crate::protocol::invalid_response;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::response::Response;

use bytes::Bytes;

/// Decodes a simple command response: expects either
/// - SUCCESS with 0 fields → success JSON
/// - ERROR with exactly one field (id=1, type=0x01 string) → error JSON with message
/// Returns appropriate Response, or invalid_response() on protocol errors
pub fn decode_simple_response(metrics: MetricsRef, payload: &Bytes) -> Response {
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
        return handle_non_success_status(metrics, data, status, field_count, offset);
    }

    // SUCCESS + 0 fields only
    if field_count == 0 {
        let rsp = br#"{"status":"success"}"#;
        metrics.add_bytes_sent(rsp.len() as u64);
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
    metrics: MetricsRef,
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
        return handle_non_success_status(metrics, data, status, field_count, offset);
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

    metrics.add_bytes_sent(json.len() as u64);

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
pub fn decode_boolean_response(metrics: MetricsRef, payload: &Bytes, field_name: &str) -> Response {
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
        return handle_non_success_status(metrics, data, status, field_count, offset);
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

    metrics.add_bytes_sent(json.len() as u64);

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
/// Handles any status other than "SUCCESS".
/// Tries to extract a meaningful error message from the first field (id=1, type=0x01),
/// otherwise falls back to generic status-based error.
pub fn handle_non_success_status(
    metrics: MetricsRef,
    data: &[u8],
    status: &[u8],
    field_count: u16,
    offset: usize,
) -> Response {
    // Try to extract error message using the shared function
    let error_msg = match extract_error_message(data, field_count, offset) {
        Ok(msg) => msg,
        Err(_) => {
            // If extraction fails, fallback to sanitized status
            let mut clean: Vec<u8> = status
                .iter()
                .copied()
                .filter(|b| *b >= 0x20 && *b <= 0x7E)
                .collect();

            if clean.is_empty() {
                clean.extend_from_slice(b"UNKNOWN_ERROR");
            }

            String::from_utf8_lossy(&clean).to_string()
        }
    };

    // Determine HTTP status code based on the error message
    let status_code = match error_msg.as_str() {
        "The requested segment was not found" => StatusCode::NOT_FOUND,
        "Service is temporarily unavailable. Please try again later." => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        "The request timed out. Please try again." => StatusCode::GATEWAY_TIMEOUT,
        "An internal error occurred. Please try again later." => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    };

    // Build JSON response
    let mut json = Vec::with_capacity(64 + error_msg.len());
    json.extend_from_slice(br#"{"status":"error","message":""#);
    json.extend_from_slice(error_msg.as_bytes());
    json.extend_from_slice(br#""}"#);

    metrics.inc_client_errors();
    metrics.add_bytes_sent(json.len() as u64);

    (
        status_code,
        [(header::CONTENT_TYPE, "application/json")],
        json,
    )
        .into_response()
}

/// Extracts error message from a non-success response.
/// Returns the extracted message as a String, or an error if extraction fails.
pub fn extract_error_message(
    data: &[u8],
    field_count: u16,
    offset: usize,
) -> Result<String, RZError> {
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
            let msg_offset = offset + 7;

            if msg_offset + field_len <= data.len() {
                let message = &data[msg_offset..msg_offset + field_len];

                if !message.is_empty() {
                    // Try to parse as UTF-8
                    if let Ok(msg_str) = std::str::from_utf8(message) {
                        // Map known error codes to user-friendly messages
                        let friendly_msg = match msg_str {
                            "404" => "The requested segment was not found",
                            "503" => "Service is temporarily unavailable. Please try again later.",
                            "408" => "The request timed out. Please try again.",
                            "500" => "An internal error occurred. Please try again later.",
                            other => other, // Use the actual message
                        };
                        return Ok(friendly_msg.to_string());
                    } else {
                        // If not valid UTF-8, try to use as is or convert
                        return Ok(String::from_utf8_lossy(message).to_string());
                    }
                }
            }
        }
    }

    // Fallback: return a generic error
    Err(RZError::Internal(
        "Failed to extract error message from response".into(),
    ))
}
