// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use bytes::{Bytes, BytesMut};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};

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

pub const ROUTER_MAGIC: u8 = 0xFE;
pub const SHARD_MAGIC: u8 = 0xFF;

/// Special control segment used for application-level keepalives.
pub const KEEPALIVE_SEGMENT: &str = "__keepalive__";

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
