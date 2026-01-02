// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::error::RZError;
use uuid::Uuid;

/// Converts a 24-bit rate feature mask into a Vec<String> of feature names
pub fn bitmask_to_rate_feature_strings(mask: u32) -> Result<Vec<String>, RZError> {
    let rate_features = super::bitmask::get_rate_features()?;

    let mut out = Vec::with_capacity(24.min(rate_features.len()));

    for i in 0..24 {
        if i >= rate_features.len() {
            break;
        }
        if (mask & (1 << i)) != 0 {
            out.push(rate_features[i].to_string());
        }
    }

    Ok(out)
}

/// Unpacks a 16-bit packed date into "YYYY-MM-DD" string
pub fn u16_to_date(packed: u16) -> Result<String, RZError> {
    let year_offset = ((packed >> 9) & 0b111) as i32;
    let month = (((packed >> 5) & 0b1111) as u8 + 1) as i32; // 1-16 → 1-12 +1
    let day = ((packed & 0b11111) as u8 + 1) as i32; // 1-31 +1

    // Base year = current year (like Go's time.Now().Year())
    let current_year = chrono::Utc::now().year();
    let target_year = current_year + year_offset;

    // Use chrono for robust date validation
    use chrono::{Datelike, NaiveDate};

    let date = NaiveDate::from_ymd_opt(target_year, month as u32, day as u32)
        .ok_or_else(|| RZError::Validation("invalid packed date".into()))?;

    // Extra validation: ensure month/day match input (catches overflow)
    if date.month() as i32 != month || date.day() as i32 != day {
        return Err(RZError::Validation("invalid packed date".into()));
    }

    Ok(date.format("%Y-%m-%d").to_string())
}

/// Converts raw property ID bytes into String
pub fn bytes_to_property_id(data: &[u8]) -> String {
    // Case 1: too short
    if data.len() < 7 {
        return String::new();
    }

    // Case 2: short string marker (0xF0 in byte 6)
    if data[6] == 0xF0 {
        let mut left_len = 0;
        for &b in &data[..6] {
            if b == 0 {
                break;
            }
            left_len += 1;
        }

        let mut right_len = 0;
        for &b in &data[7..] {
            if b == 0 {
                break;
            }
            right_len += 1;
        }

        let mut result = Vec::with_capacity(left_len + right_len);
        result.extend_from_slice(&data[..left_len]);
        result.extend_from_slice(&data[7..7 + right_len]);
        return String::from_utf8_lossy(&result).to_string();
    }

    // Case 3: UUID detection (valid version in high nibble of byte 6)
    let version = (data[6] & 0xF0) >> 4;
    if matches!(version, 1 | 2 | 3 | 4 | 5 | 7) {
        let mut uuid_bytes = [0u8; 16];
        let copy_len = data.len().min(16);
        uuid_bytes[..copy_len].copy_from_slice(&data[..copy_len]);

        let u = Uuid::from_bytes(uuid_bytes);
        return u.to_string();
    }

    // Fallback — should never happen with valid server data
    String::new()
}

// ---------------------------------
// ---------------------------------

use chrono::{Datelike, NaiveDate, Utc};

/// Writes a packed u16 date directly as "YYYY-MM-DD" into the JSON buffer.
/// Performs full validation (including leap years) but produces no String.
pub fn write_packed_date(json: &mut Vec<u8>, packed: u16) -> Result<(), RZError> {
    let year_offset = ((packed >> 9) & 0b111) as i32;
    let month = ((packed >> 5) & 0b1111) as u32 + 1; // 0..15 → 1..16
    let day = (packed & 0b11111) as u32 + 1; // 0..31 → 1..32

    // Fast pre-check before chrono
    if month > 12 || day > 31 {
        return Err(RZError::Validation(
            "invalid packed date: month/day out of range".into(),
        ));
    }

    let current_year = Utc::now().year();
    let target_year = current_year + year_offset;

    // Validate with chrono for leap years and month lengths
    let date = NaiveDate::from_ymd_opt(target_year, month, day)
        .ok_or_else(|| RZError::Validation("invalid packed date".into()))?;

    if date.month() != month || date.day() != day {
        return Err(RZError::Validation("invalid packed date: overflow".into()));
    }

    // Manually write YYYY-MM-DD (fast, no allocation)
    write_u32_four_digits(json, target_year as u32);
    json.push(b'-');
    write_u32_two_digits(json, month);
    json.push(b'-');
    write_u32_two_digits(json, day);

    Ok(())
}

/// Writes a 4-digit year (1000..9999 assumed safe range)
#[inline]
fn write_u32_four_digits(buf: &mut Vec<u8>, mut n: u32) {
    // Unrolled for speed
    buf.push(b'0' + (n / 1000) as u8);
    n %= 1000;
    buf.push(b'0' + (n / 100) as u8);
    n %= 100;
    buf.push(b'0' + (n / 10) as u8);
    buf.push(b'0' + (n % 10) as u8);
}

/// Writes a 2-digit number with leading zero (01..12, 01..31)
#[inline]
fn write_u32_two_digits(buf: &mut Vec<u8>, mut n: u32) {
    if n < 10 {
        buf.push(b'0');
    } else {
        buf.push(b'0' + (n / 10) as u8);
        n %= 10;
    }
    buf.push(b'0' + n as u8);
}

/// Writes a quoted property_id directly into the JSON buffer.
/// Handles both short string format and UUID format without any allocation.
pub fn write_quoted_property_id(json: &mut Vec<u8>, data: &[u8]) -> Result<(), RZError> {
    // DO NOT push opening quote here!

    if data.len() < 7 {
        // Invalid → write empty string: ""
        json.extend_from_slice(br#""""#);
        return Ok(());
    }

    if data[6] == 0xF0 {
        // Short string format: left [0..6] null-terminated + right [7..] null-terminated
        json.push(b'"'); // ← opening quote

        // Left part
        for &b in &data[..6] {
            if b == 0 {
                break;
            }
            json.push(b); // safe: property IDs are printable ASCII
        }

        // Right part (if exists)
        if data.len() >= 8 {
            for &b in &data[7..] {
                if b == 0 {
                    break;
                }
                json.push(b);
            }
        }

        json.push(b'"'); // ← closing quote
        return Ok(());
    }

    // UUID format
    let version = (data[6] & 0xF0) >> 4;
    if matches!(version, 1 | 2 | 3 | 4 | 5 | 7) {
        let mut uuid_bytes = [0u8; 16];
        let copy_len = data.len().min(16);
        uuid_bytes[..copy_len].copy_from_slice(&data[..copy_len]);

        json.push(b'"'); // ← opening quote

        const HEX: [u8; 16] = *b"0123456789abcdef";

        macro_rules! hex2 {
            ($b:expr) => {{
                json.push(HEX[($b >> 4) as usize]);
                json.push(HEX[($b & 0x0F) as usize]);
            }};
        }

        hex2!(uuid_bytes[0]);
        hex2!(uuid_bytes[1]);
        hex2!(uuid_bytes[2]);
        hex2!(uuid_bytes[3]);
        json.push(b'-');
        hex2!(uuid_bytes[4]);
        hex2!(uuid_bytes[5]);
        json.push(b'-');
        hex2!(uuid_bytes[6]);
        hex2!(uuid_bytes[7]);
        json.push(b'-');
        hex2!(uuid_bytes[8]);
        hex2!(uuid_bytes[9]);
        json.push(b'-');
        hex2!(uuid_bytes[10]);
        hex2!(uuid_bytes[11]);
        hex2!(uuid_bytes[12]);
        hex2!(uuid_bytes[13]);
        hex2!(uuid_bytes[14]);
        hex2!(uuid_bytes[15]);

        json.push(b'"'); // ← closing quote

        return Ok(());
    }

    // Fallback: invalid → empty string
    json.extend_from_slice(br#""""#);
    Ok(())
}

/// Writes the "rate_feature":[...] JSON array directly into the buffer.
/// Zero allocations, handles uninitialized features gracefully.
/// Highly optimized: writes "rate_feature":[...] with zero bounds checks after init
/// Assumes RATE_FEATURES is initialized (which it is in hot path)
pub fn write_rate_features_array(json: &mut Vec<u8>, mask: u32) {
    json.extend_from_slice(br#""rate_feature":["#);

    if let Some(features) = super::bitmask::RATE_FEATURES.get() {
        let mut wrote_any = false;
        let mut bit = 1u32;
        for &feature in features.iter().take(24) {
            if mask & bit != 0 {
                if wrote_any {
                    json.push(b',');
                }
                wrote_any = true;
                json.push(b'"');
                json.extend_from_slice(feature.as_bytes());
                json.push(b'"');
            }
            bit <<= 1;
        }
    }

    json.extend_from_slice(br#"]"#);
}
