// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use bytes::Bytes;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task;

async fn process_search_avail(payload: &Value) -> Vec<u8> {
    let segment = payload["segment"].as_str().unwrap_or("");
    let room_type = payload["room_type"].as_str().unwrap_or("");

    let area = payload["area"].as_str();
    let property_id = payload["property_id"].as_str();
    let property_type = payload["type"].as_str();
    let stars = payload["stars"].as_u64().map(|s| s as u8);
    let category = payload["category"].as_str();
    let amenities = payload["amenities"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(",")
    });
    let longitude = payload["longitude"].as_f64();
    let latitude = payload["latitude"].as_f64();
    let date = payload["date"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(",")
    });
    let availability = payload["availability"].as_u64().map(|a| a as u8);
    let final_price = payload["final_price"].as_u64().map(|v| v as u32);
    let rate_feature = payload["rate_features"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(",")
    });
    let limit = payload["limit"].as_u64();

    let mut buf = Vec::new();
    let cmd_name = "SEARCHAVAIL";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());
    let field_count_pos = buf.len();
    buf.extend_from_slice(&0u16.to_le_bytes());
    let mut field_count = 0u16;

    let mut add_field = |id: u16, typ: u8, data: &[u8]| {
        buf.extend_from_slice(&id.to_le_bytes());
        buf.push(typ);
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(data);
        field_count += 1;
    };

    add_field(0x01, 0x01, segment.as_bytes());
    add_field(0x02, 0x01, room_type.as_bytes());

    if let Some(a) = area {
        add_field(0x03, 0x01, a.as_bytes());
    }
    if let Some(p) = property_id {
        add_field(0x04, 0x01, p.as_bytes());
    }
    if let Some(t) = property_type {
        add_field(0x05, 0x01, t.as_bytes());
    }
    if let Some(s) = stars {
        add_field(0x06, 0x02, &[s]);
    }
    if let Some(c) = category {
        add_field(0x07, 0x01, c.as_bytes());
    }
    if let Some(a) = amenities.as_ref() {
        add_field(0x08, 0x01, a.as_bytes());
    }
    if let Some(lon) = longitude {
        add_field(0x09, 0x03, &lon.to_le_bytes());
    }
    if let Some(lat) = latitude {
        add_field(0x0A, 0x03, &lat.to_le_bytes());
    }
    if let Some(d) = date.as_ref() {
        add_field(0x0B, 0x01, d.as_bytes());
    }
    if let Some(a) = availability {
        add_field(0x0C, 0x02, &[a]);
    }
    if let Some(p) = final_price {
        add_field(0x0D, 0x03, &p.to_le_bytes());
    }
    if let Some(r) = rate_feature.as_ref() {
        add_field(0x0E, 0x01, r.as_bytes());
    }
    if let Some(l) = limit {
        add_field(0x0F, 0x03, &l.to_le_bytes());
    }

    buf[field_count_pos..field_count_pos + 2].copy_from_slice(&field_count.to_le_bytes());
    buf
}

fn decode_search_avail_response(payload: &Bytes) -> Vec<u8> {
    let data = payload.as_ref();
    if data.is_empty() {
        return b"invalid".to_vec();
    }

    let status_len = data[0] as usize;
    let min_header_len = 1 + status_len + 2;
    if data.len() < min_header_len {
        return b"invalid".to_vec();
    }

    let status = &data[1..1 + status_len];
    if status != b"SUCCESS" {
        return b"non-success".to_vec();
    }

    let total_fields = u16::from_le_bytes([data[1 + status_len], data[1 + status_len + 1]]);
    let mut offset = 1 + status_len + 2;

    if total_fields == 0 {
        return br#"{"status":"success","properties":[]}"#.to_vec();
    }

    if total_fields < 1 || (total_fields - 1) % 2 != 0 {
        return b"invalid field count".to_vec();
    }

    let mut json = Vec::with_capacity(32768);
    json.extend_from_slice(br#"{"status":"success","properties":["#);

    let mut current_field: u16 = 1;
    let mut first_property = true;

    while offset + 7 <= data.len() {
        let field_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        if field_id != current_field {
            return b"wrong field order".to_vec();
        }

        let field_type = data[offset + 2];
        let field_len = u32::from_le_bytes([
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
        ]) as usize;

        offset += 7;
        if offset + field_len > data.len() {
            return b"overflow".to_vec();
        }

        let field_data = &data[offset..offset + field_len];
        offset += field_len;

        match (current_field, field_type) {
            (1, 0x02) => {
                if field_len != 2 {
                    return b"bad num_days".to_vec();
                }
                // We don't need num_days value for JSON building, just validate later per property
            }
            (id, 0x01) if id % 2 == 0 && id > 1 => {
                if !first_property {
                    json.push(b',');
                }
                first_property = false;

                json.push(b'{');
                json.extend_from_slice(br#""property_id":"#);
                if write_quoted_property_id(&mut json, field_data).is_err() {
                    return b"bad prop id".to_vec();
                }
                json.extend_from_slice(br#","days":["#);

                // Next field must be days vector (type 0x08)
                if offset + 7 > data.len() {
                    return b"missing days".to_vec();
                }
                let next_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
                if next_id != current_field + 1 {
                    return b"days id mismatch".to_vec();
                }
                let days_type = data[offset + 2];
                let days_len = u32::from_le_bytes([
                    data[offset + 3],
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                ]) as usize;

                offset += 7;
                if days_type != 0x08 || offset + days_len > data.len() {
                    return b"bad days header".to_vec();
                }

                let days_data = &data[offset..offset + days_len];
                offset += days_len;

                if days_len < 2 {
                    return b"days too short".to_vec();
                }

                let days_count = u16::from_le_bytes([days_data[0], days_data[1]]);
                let expected_len = 2 + 11 * days_count as usize;
                if days_len != expected_len {
                    return b"days length mismatch".to_vec();
                }

                let mut first_day = true;
                let mut d_off = 2;
                for _ in 0..days_count {
                    if !first_day {
                        json.push(b',');
                    }
                    first_day = false;
                    json.push(b'{');

                    json.extend_from_slice(br#""date":""#);
                    let date_packed = u16::from_le_bytes([days_data[d_off], days_data[d_off + 1]]);
                    if write_packed_date(&mut json, date_packed).is_err() {
                        return b"bad date".to_vec();
                    }

                    let availability = days_data[d_off + 2];
                    json.extend_from_slice(br#"","availability":"#);
                    json.extend_from_slice(itoa::Buffer::new().format(availability).as_bytes());

                    let final_price = u32::from_le_bytes([
                        days_data[d_off + 3],
                        days_data[d_off + 4],
                        days_data[d_off + 5],
                        days_data[d_off + 6],
                    ]);
                    json.extend_from_slice(br#","final_price":"#);
                    json.extend_from_slice(itoa::Buffer::new().format(final_price).as_bytes());

                    let rate_mask = u32::from_le_bytes([
                        days_data[d_off + 7],
                        days_data[d_off + 8],
                        days_data[d_off + 9],
                        days_data[d_off + 10],
                    ]);
                    json.push(b',');
                    write_rate_features_array(&mut json, rate_mask);

                    json.push(b'}');
                    d_off += 11;
                }

                json.extend_from_slice(br#"]}"#); // close days[] and property
                current_field += 1; // skip days field
            }
            _ => return b"unexpected field".to_vec(),
        }
        current_field += 1;
    }

    if (current_field - 1) != total_fields {
        return b"fields not consumed".to_vec();
    }

    json.extend_from_slice(br#"]}"#); // close properties[] and root
    json
}

#[tokio::main]
async fn main() {
    let input_json = json!({
        "segment": "segment_1",
        "room_type": "room_1",
        "area": "area_1",
        "property_id": "prop123",
        "type": "hotel",
        "stars": 5,
        "category": "test",
        "amenities": ["wifi", "pool", "parking"],
        "longitude": 10.123,
        "latitude": 53.55,
        "date": ["2026-01-10", "2026-01-11", "2026-01-12"],
        "availability": 1,
        "final_price": 4000,
        "rate_features": ["free_cancellation", "pay_at_property"],
        "limit": 300
    });

    // Realistic fake binary: 3 properties, each with 7 days
    let fake_binary = {
        let mut buf = Vec::new();
        buf.push(7); // "SUCCESS"
        buf.extend_from_slice(b"SUCCESS");
        buf.extend_from_slice(&7u16.to_le_bytes()); // total_fields = 1 (num_days) + 3*(prop_id + days) = 7

        // field 1: num_days = 7
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.push(0x02);
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&7u16.to_le_bytes());

        for prop_idx in 1..=3 {
            // property_id
            let prop_id = format!("prop_{}", prop_idx);
            buf.extend_from_slice(&(2 + 2 * (prop_idx - 1) as u16).to_le_bytes());
            buf.push(0x01);
            buf.extend_from_slice(&(prop_id.len() as u32).to_le_bytes());
            buf.extend_from_slice(prop_id.as_bytes());

            // days vector: 7 days
            buf.extend_from_slice(&(3 + 2 * (prop_idx - 1) as u16).to_le_bytes());
            buf.push(0x08);
            let days_len = 2 + 11 * 7;
            buf.extend_from_slice(&(days_len as u32).to_le_bytes());
            buf.extend_from_slice(&7u16.to_le_bytes()); // count

            for day in 0..7 {
                buf.extend_from_slice(
                    &((2026 - 1900) as u16 * 512 + (1 + day) * 32 + 10).to_le_bytes(),
                ); // ~2026-01-10 + day
                buf.push(3 + day as u8); // availability
                buf.extend_from_slice(&((100 + day * 20) as u32).to_le_bytes()); // price
                buf.extend_from_slice(&((1 << day) as u32).to_le_bytes()); // varying rate_mask
            }
        }

        Bytes::from(buf)
    };

    println!("=== SearchAvail Conversion Benchmark ===");
    println!("Input JSON: {} bytes", input_json.to_string().len());
    println!(
        "Fake binary response: {} bytes (3 properties × 7 days)\n",
        fake_binary.len()
    );

    let concurrencies = [1, 4, 8, 16, 32, 64, 128];

    for &concurrency in &concurrencies {
        run_benchmark(concurrency, &input_json, &fake_binary).await;
    }
}

async fn run_benchmark(concurrency: usize, input_json: &Value, fake_binary: &Bytes) {
    let start = Instant::now();
    let duration = Duration::from_secs(5);
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handles = Vec::new();

    for _ in 0..concurrency {
        let counter = counter.clone();
        let input = input_json.clone();
        let binary = fake_binary.clone();

        handles.push(task::spawn(async move {
            let mut local_count = 0;
            while start.elapsed() < duration {
                let _bin_out = process_search_avail(&input).await;
                let _json_out = decode_search_avail_response(&binary);
                local_count += 1;
            }
            counter.fetch_add(local_count, std::sync::atomic::Ordering::Relaxed);
        }));
    }

    tokio::time::sleep(duration).await;
    for h in handles {
        h.await.ok();
    }

    let total = counter.load(std::sync::atomic::Ordering::Relaxed);
    let tps = total as f64 / duration.as_secs_f64();

    println!(
        "Concurrency: {:3} │ Ops: {:8} │ Throughput: {:7.0} full conversions/sec",
        concurrency, total, tps
    );
}

use chrono::{Datelike, NaiveDate, Utc};
// In crate::helper (or wherever these live)

use std::error::Error;

/// Writes a packed u16 date directly as "YYYY-MM-DD" into the JSON buffer.
/// Returns a boxed error on invalid date.
pub fn write_packed_date(json: &mut Vec<u8>, packed: u16) -> Result<(), Box<dyn Error>> {
    let year_offset = ((packed >> 9) & 0b111) as i32;
    let month = ((packed >> 5) & 0b1111) as u32 + 1; // 0..15 → 1..16
    let day = (packed & 0b11111) as u32 + 1; // 0..31 → 1..32

    if month > 12 || day > 31 {
        return Err("invalid packed date: month/day out of range".into());
    }

    let current_year = Utc::now().year();
    let target_year = current_year + year_offset;

    let date = NaiveDate::from_ymd_opt(target_year, month, day)
        .ok_or("invalid packed date: rejected by chrono")?;

    // Extra safety: chrono normalizes overflowing days (e.g., Jan 32 → Feb 1)
    if date.month() != month || date.day() != day {
        return Err("invalid packed date: day/month overflow".into());
    }

    write_u32_four_digits(json, target_year as u32);
    json.push(b'-');
    write_u32_two_digits(json, month);
    json.push(b'-');
    write_u32_two_digits(json, day);

    Ok(())
}

#[inline]
fn write_u32_four_digits(buf: &mut Vec<u8>, mut n: u32) {
    buf.push(b'0' + (n / 1000) as u8);
    n %= 1000;
    buf.push(b'0' + (n / 100) as u8);
    n %= 100;
    buf.push(b'0' + (n / 10) as u8);
    buf.push(b'0' + (n % 10) as u8);
}

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
/// Returns a boxed error only if something truly unexpected happens (rare).
/// On malformed but recoverable input, falls back to empty string.
pub fn write_quoted_property_id(json: &mut Vec<u8>, data: &[u8]) -> Result<(), Box<dyn Error>> {
    if data.len() < 7 {
        json.extend_from_slice(br#""""#);
        return Ok(());
    }

    if data[6] == 0xF0 {
        // Short string format
        json.push(b'"');
        for &b in &data[..6] {
            if b == 0 {
                break;
            }
            json.push(b);
        }
        if data.len() >= 8 {
            for &b in &data[7..] {
                if b == 0 {
                    break;
                }
                json.push(b);
            }
        }
        json.push(b'"');
        return Ok(());
    }

    // UUID format
    let version = (data[6] & 0xF0) >> 4;
    if matches!(version, 1 | 2 | 3 | 4 | 5 | 7) {
        let mut uuid_bytes = [0u8; 16];
        let copy_len = data.len().min(16);
        uuid_bytes[..copy_len].copy_from_slice(&data[..copy_len]);

        json.push(b'"');
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
        json.push(b'"');
        return Ok(());
    }

    // Unknown/invalid format → safe fallback
    json.extend_from_slice(br#""""#);
    Ok(())
}
/// Static list of 24 possible rate features, in bit order (bit 0 = index 0, etc.)
/// These are common real-world hotel rate features — adjust if your production list differs.
pub static RATE_FEATURES: [&str; 24] = [
    "free_cancellation",
    "non_refundable",
    "pay_at_property",
    "no_prepayment",
    "breakfast_included",
    "free_breakfast",
    "half_board",
    "full_board",
    "all_inclusive",
    "free_wifi",
    "parking_included",
    "free_parking",
    "pet_friendly",
    "no_pets",
    "smoking_room",
    "non_smoking",
    "family_room",
    "accessible_room",
    "pool_access",
    "gym_access",
    "spa_included",
    "late_checkout",
    "early_checkin",
    "mobile_checkin",
];

/// Writes the "rate_feature":[...] JSON array directly into the buffer.
/// Zero allocations, highly optimized, no Option unwrapping.
/// Assumes mask bits correspond to RATE_FEATURES indices (0..23)
pub fn write_rate_features_array(json: &mut Vec<u8>, mask: u32) {
    json.extend_from_slice(br#""rate_feature":["#);

    let mut wrote_any = false;
    let mut bit = 1u32;

    // Unrolled loop over static array — compiler can optimize heavily
    for &feature in RATE_FEATURES.iter() {
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
        // Safety: we stop at 24, so bit never overflows u32 meaningfully here
    }

    json.extend_from_slice(br#"]"#);
}
