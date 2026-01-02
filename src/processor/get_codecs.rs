// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use bytes::Bytes;

// Adjust these imports to match your project structure
use crate::{
    error::RZError,
    handler::{handler::Handler, protocol::ProtocolError},
    processor::base::Codecs,
};

pub async fn process_get_codecs(handler: &Handler) -> Result<Codecs, RZError> {
    // Build payload: GETCODECS with 0 fields
    let mut buf = Vec::new();

    let cmd_name = "GETCODECS";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());

    // Field count = 0
    buf.extend_from_slice(&0u16.to_le_bytes());

    // Execute — note: this is internal, so we use the raw field data
    let field_data = handler.execute(false, buf).await?;

    decode_get_codecs_response(&field_data).map_err(|e| RZError::ParseError(e.to_string()))
}

pub fn decode_get_codecs_response(payload: &Bytes) -> Result<Codecs, ProtocolError> {
    let data = payload.as_ref();

    if data.is_empty() {
        return Err(ProtocolError::InvalidResponse(
            "empty payload for GETCODECS".into(),
        ));
    }

    let status_len = data[0] as usize;
    let min_len = 1 + status_len + 2;
    if data.len() < min_len {
        return Err(ProtocolError::InvalidResponse(
            "payload too short for status + field count".into(),
        ));
    }

    let status = &data[1..1 + status_len];
    let field_count = u16::from_le_bytes([data[1 + status_len], data[1 + status_len + 1]]);

    let mut offset = 1 + status_len + 2;

    // --- Defensive check: must be SUCCESS ---
    if status != b"SUCCESS" {
        return Err(ProtocolError::InvalidResponse(
            format!("unexpected status: {}", String::from_utf8_lossy(status)).into(),
        ));
    }

    // --- Must have exactly 1 field ---
    if field_count != 1 {
        return Err(ProtocolError::InvalidResponse(
            format!("expected exactly 1 field, got {}", field_count).into(),
        ));
    }

    if offset + 7 > data.len() {
        return Err(ProtocolError::InvalidResponse(
            "truncated field header".into(),
        ));
    }

    let field_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
    let field_type = data[offset + 2];

    if field_id != 1 {
        return Err(ProtocolError::InvalidResponse(
            format!("expected field ID 1, got {}", field_id).into(),
        ));
    }

    let field_len = u32::from_le_bytes([
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
    ]) as usize;
    offset += 7;

    if offset + field_len > data.len() {
        return Err(ProtocolError::InvalidResponse(
            "truncated field data".into(),
        ));
    }

    let field_data = &data[offset..offset + field_len];

    // --- Error case: type 0x01 (string message) ---
    if field_type == 0x01 {
        let msg = std::str::from_utf8(field_data)
            .unwrap_or("invalid UTF-8 error message")
            .to_string();
        return Err(ProtocolError::InvalidResponse(msg));
    }

    // --- Success case: type 0x09, comma-separated rate features ---
    if field_type != 0x09 {
        return Err(ProtocolError::InvalidResponse(
            format!("expected field type 0x09, got 0x{:02x}", field_type).into(),
        ));
    }

    let raw_str = std::str::from_utf8(field_data)
        .map_err(|_| ProtocolError::InvalidResponse("invalid UTF-8 in codecs payload".into()))?;

    let rate_features: Vec<String> = raw_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(Codecs { rate_features })
}
