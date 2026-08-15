use crate::metrics::MetricsEvent;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::response::Response;
use tokio::sync::mpsc::Sender;

use bytes::{Bytes, BytesMut};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

pub const STATUS_KEY: &[u8] = br#""status":"#;
pub const MESSAGE_KEY: &[u8] = br#""message":"#;
pub const ERROR_VALUE: &[u8] = br#""error""#;
pub const SUCCESS_VALUE: &[u8] = br#""success""#;
pub const SHARD_MAGIC: u8 = 0xFF;
pub const ROUTER_MAGIC: u8 = 0xFE;

/// Special control segment used for application-level keepalives.
pub const KEEPALIVE_SEGMENT: &str = "__keepalive__";

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("incomplete frame")]
    ShortFrame,
    #[error("missing magic byte: got 0x{0:02x}")]
    MissingMagic(u8),
    #[error("short payload: {0}")]
    ShortPayload(String),
    #[error("extra {0} bytes after parsing fields")]
    ExtraBytes(usize),
    #[error("invalid UTF-8 in status string")]
    InvalidUtf8,
    #[error("invalid response: {0}")]
    InvalidResponse(String),
}

// Header is the decoded fixed part of the frame.
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub clr_id: u32,
    pub status_len: u8, // "SUCCESS" or "ERROR"
    pub field_cnt: u16, // number of fields that follow
}

pub async fn drain_frame_async(
    reader: &mut (impl AsyncRead + Unpin),
    buf: &mut BytesMut,
) -> Result<(Header, Bytes), ProtocolError> {
    // Read header (9 bytes)
    while buf.len() < 9 {
        if reader
            .read_buf(buf)
            .await
            .map_err(|_| ProtocolError::ShortFrame)?
            == 0
        {
            return Err(ProtocolError::ShortFrame);
        }
    }

    let header_bytes = &buf[..9];
    if header_bytes[0] != 0xFF {
        return Err(ProtocolError::MissingMagic(header_bytes[0]));
    }

    let clr_id = u32::from_le_bytes(header_bytes[1..5].try_into().unwrap());
    let payload_len = u32::from_le_bytes(header_bytes[5..9].try_into().unwrap()) as usize;

    // Read full payload
    while buf.len() < 9 + payload_len {
        if reader
            .read_buf(buf)
            .await
            .map_err(|_| ProtocolError::ShortFrame)?
            == 0
        {
            return Err(ProtocolError::ShortFrame);
        }
    }

    // Explicitly discard the header part — we don't need it
    _ = buf.split_to(9);

    // Take the payload
    let payload_mut = buf.split_to(payload_len);
    let payload = payload_mut.freeze();

    if payload.is_empty() {
        return Err(ProtocolError::ShortPayload("empty payload".into()));
    }

    let status_len = payload[0] as usize;
    if payload.len() < 1 + status_len + 2 {
        return Err(ProtocolError::ShortPayload("missing field count".into()));
    }

    let field_cnt = u16::from_le_bytes(
        payload[1 + status_len..1 + status_len + 2]
            .try_into()
            .unwrap(),
    );

    let hdr = Header {
        clr_id,
        status_len: payload[0],
        field_cnt,
    };

    Ok((hdr, payload))
}

/// Build a router-level keepalive frame (segment = "__keepalive__").
/// clrid can be 0 – it is a control message, not demuxed.
pub fn build_keepalive_frame() -> Vec<u8> {
    let segment = KEEPALIVE_SEGMENT.as_bytes();
    let seg_len = segment.len() as u8;

    // total_len = seg_len_byte + segment + is_write + shard header (1 + 4 + 4)
    let total_len = 1 + segment.len() + 1 + 9;

    let mut buf = Vec::with_capacity(5 + total_len);
    buf.push(ROUTER_MAGIC);
    buf.extend_from_slice(&(total_len as u32).to_le_bytes());
    buf.push(seg_len);
    buf.extend_from_slice(segment);
    buf.push(0x00); // is_write = false
    buf.push(SHARD_MAGIC);
    buf.extend_from_slice(&0u32.to_le_bytes()); // clrid = 0
    buf.extend_from_slice(&0u32.to_le_bytes()); // empty payload length / padding
    buf
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

/// Builds a router frame for cluster mode
/// | routerMagic(1) | totalLen(4) | segmentLen(1) | segment(n) | isWrite(1) | shardFrame |
/// where shardFrame is the output of prepend_header
pub fn prepend_router_header(segment: &str, is_write: bool, clrid: u32, payload: &[u8]) -> Vec<u8> {
    // First build the shard frame to know its length
    let shard_total_len = payload.len() as u32;
    let shard_frame_len = 9 + shard_total_len; // magic(1) + clrid(4) + totalLen(4) + payload

    // Router header components
    let segment_len = segment.len() as u32;
    let router_header_len = 1 + segment_len + 1; // segmentLen(1) + segment(n) + isWrite(1)

    // Total frame length: routerMagic(1) + totalLen(4) + routerHeader + shardFrame
    let total_len = 1 + 4 + router_header_len + shard_frame_len;

    let mut out = Vec::with_capacity(total_len as usize);

    // Router magic byte
    out.push(ROUTER_MAGIC);

    // Total length (everything after this field)
    out.extend_from_slice(&((router_header_len + shard_frame_len) as u32).to_le_bytes());

    // Segment length
    out.push(segment_len as u8);

    // Segment
    out.extend_from_slice(segment.as_bytes());

    // IsWrite flag
    out.push(if is_write { 0x01 } else { 0x00 });

    // Shard frame (magic, clrid, totalLen, payload)
    out.push(SHARD_MAGIC);
    out.extend_from_slice(&clrid.to_le_bytes());
    out.extend_from_slice(&shard_total_len.to_le_bytes());
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
