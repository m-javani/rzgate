// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use bytes::{Bytes, BytesMut};
use std::io::{self, Write};
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

pub fn prepend_header(clr_id: u32, payload: &[u8]) -> Vec<u8> {
    let total_len = payload.len() as u32;
    let mut out = Vec::with_capacity(9 + total_len as usize);
    out.push(0xFF);
    out.extend_from_slice(&clr_id.to_le_bytes());
    out.extend_from_slice(&total_len.to_le_bytes());
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

pub fn build_login_payload(token: &str) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();

    let cmd = "LOGIN";
    buf.write_all(&[(cmd.len() as u8)])?; // cmd len
    buf.write_all(cmd.as_bytes())?; // cmd name
    buf.write_all(&1u16.to_le_bytes())?; // field count = 1
    buf.write_all(&0x01u16.to_le_bytes())?; // field ID
    buf.write_all(&[0x01])?; // field type (string)
    buf.write_all(&(token.len() as u32).to_le_bytes())?; // token len
    buf.write_all(token.as_bytes())?; // token

    Ok(buf)
}

pub fn parse_field_slice<'a>(data: &'a [u8], field_cnt: u16) -> Result<&'a [u8], ProtocolError> {
    let mut pos = 0usize;

    for _ in 0..field_cnt {
        // Field ID (2 bytes)
        if pos + 2 > data.len() {
            return Err(ProtocolError::ShortFrame);
        }
        let _id = u16::from_le_bytes(data[pos..pos + 2].try_into().unwrap());
        pos += 2;

        // Field type (1 byte)
        if pos >= data.len() {
            return Err(ProtocolError::ShortFrame);
        }
        let _typ = data[pos];
        pos += 1;

        // Field length (4 bytes)
        if pos + 4 > data.len() {
            return Err(ProtocolError::ShortFrame);
        }
        let len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;

        // Field data
        if pos + len > data.len() {
            return Err(ProtocolError::ShortPayload("field overflow".into()));
        }
        pos += len;
    }

    if pos != data.len() {
        return Err(ProtocolError::ExtraBytes(data.len() - pos));
    }

    Ok(data)
}
