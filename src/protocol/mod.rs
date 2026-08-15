pub mod response;

use crate::error::RZError;
use crate::metrics::MetricsRef;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::response::Response;

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

fn build_error_message(error: RZError) -> String {
    match error {
        // Client errors - return the actual message
        RZError::Validation(msg) => msg,

        // Upstream/System errors - return generic user-friendly messages
        RZError::RoomzinUnreachable(_) => {
            "Service is temporarily unavailable. Please try again later.".into()
        }
        RZError::Network(_) => {
            "Unable to reach the service. Please check your connection and retry.".into()
        }
        RZError::Timeout => "The request took too long. Please try again.".into(),
        RZError::Internal(_) => "Something went wrong on our end. Please try again later.".into(),
    }
}

pub async fn error_response(metrics: MetricsRef, error: RZError) -> Response {
    metrics.inc_client_errors();

    let message = build_error_message(error);

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

pub fn invalid_response() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [(header::CONTENT_TYPE, "application/json")],
        br#"{"status":"error","message":"INVALID_RESPONSE_FORMAT"}"#,
    )
        .into_response()
}
