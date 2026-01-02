// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::helper::{write_packed_date, write_quoted_property_id, write_rate_features_array};
use crate::metrics::MetricsEvent;
use crate::processor::base::{handle_non_success_status, invalid_response};
use crate::{handler::handler::Handler, processor::base::error_response};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc::Sender;

pub async fn process_search_avail(
    payload: &Value,
    handler: &Handler,
    metrics_tx: Sender<MetricsEvent>,
) -> Response {
    // Required fields
    let segment = match payload.get("segment").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics_tx.clone(), "segment is required").await,
    };
    let room_type = match payload.get("room_type").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return error_response(metrics_tx.clone(), "room_type is required").await,
    };

    // Optional fields
    let area = payload.get("area").and_then(|v| v.as_str());
    let property_id = payload.get("property_id").and_then(|v| v.as_str());
    let property_type = payload.get("type").and_then(|v| v.as_str());
    let stars = payload
        .get("stars")
        .and_then(|v| v.as_u64())
        .filter(|&s| s <= 255)
        .map(|s| s as u8);
    let category = payload.get("category").and_then(|v| v.as_str());

    let amenities = match payload.get("amenities").and_then(|v| v.as_array()) {
        Some(arr) => {
            let items: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            if items.is_empty() {
                None
            } else {
                Some(items.join(","))
            }
        }
        None => None,
    };

    let longitude = payload.get("longitude").and_then(|v| v.as_f64());
    let latitude = payload.get("latitude").and_then(|v| v.as_f64());

    let date = match payload.get("date").and_then(|v| v.as_array()) {
        Some(arr) => {
            let items: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            if items.is_empty() {
                None
            } else {
                Some(items.join(","))
            }
        }
        None => None,
    };

    let availability = payload
        .get("availability")
        .and_then(|v| v.as_u64())
        .filter(|&a| a <= 255)
        .map(|a| a as u8);
    let final_price = payload
        .get("final_price")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let rate_feature = match payload.get("rate_features").and_then(|v| v.as_array()) {
        Some(arr) => {
            let items: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            if items.is_empty() {
                None
            } else {
                Some(items.join(","))
            }
        }
        None => None,
    };
    let limit = payload.get("limit").and_then(|v| v.as_u64());

    // Build payload
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

    // Required
    add_field(0x01, 0x01, segment.as_bytes());
    add_field(0x02, 0x01, room_type.as_bytes());

    // Optional
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
    if let Some(a) = amenities {
        add_field(0x08, 0x01, a.as_bytes());
    }
    if let Some(lon) = longitude {
        add_field(0x09, 0x03, &lon.to_le_bytes());
    }
    if let Some(lat) = latitude {
        add_field(0x0A, 0x03, &lat.to_le_bytes());
    }
    if let Some(d) = date {
        add_field(0x0B, 0x01, d.as_bytes());
    }
    if let Some(a) = availability {
        add_field(0x0C, 0x02, &[a]);
    }
    if let Some(p) = final_price {
        add_field(0x0D, 0x03, &p.to_le_bytes());
    }
    if let Some(r) = rate_feature {
        add_field(0x0E, 0x01, r.as_bytes());
    }
    if let Some(l) = limit {
        add_field(0x0F, 0x03, &l.to_le_bytes());
    }

    // Patch field count
    buf[field_count_pos..field_count_pos + 2].copy_from_slice(&field_count.to_le_bytes());

    match handler.execute(false, buf).await {
        Ok(field_data) => decode_search_avail_response(metrics_tx.clone(), &field_data),
        Err(e) => error_response(metrics_tx.clone(), &e.to_string()).await,
    }
}

fn decode_search_avail_response(metrics_tx: Sender<MetricsEvent>, payload: &Bytes) -> Response {
    let data = payload.as_ref();
    if data.is_empty() {
        return invalid_response();
    }

    let status_len = data[0] as usize;
    let min_header_len = 1 + status_len + 2;
    if data.len() < min_header_len {
        return invalid_response();
    }

    let status = &data[1..1 + status_len];
    let total_fields = u16::from_le_bytes([data[1 + status_len], data[1 + status_len + 1]]);
    let mut offset = 1 + status_len + 2;

    // Handle non-SUCCESS early
    if status != b"SUCCESS" {
        return handle_non_success_status(metrics_tx.clone(), data, status, total_fields, offset);
    }

    // --- SUCCESS path: streaming JSON build ---
    let mut json = Vec::with_capacity(payload.len() * 2);
    json.extend_from_slice(br#"{"status":"success","properties":["#);

    if total_fields == 0 {
        json.extend_from_slice(br#"]}"#);
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            json,
        )
            .into_response();
    }

    // Validate field count: must have at least num_days, then even number of (prop_id, days)
    if total_fields < 1 || (total_fields - 1) % 2 != 0 {
        return invalid_response();
    }

    let mut current_field: u16 = 1;
    let mut first_property = true;

    while offset + 7 <= data.len() {
        let field_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        if field_id != current_field {
            return invalid_response();
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
            return invalid_response();
        }

        let field_data = &data[offset..offset + field_len];
        offset += field_len;

        match (current_field, field_type) {
            (1, 0x02) => {
                if field_len != 2 {
                    return invalid_response();
                }
                // num_days is parsed but not stored — validated per property later
                let _num_days = u16::from_le_bytes([field_data[0], field_data[1]]);
            }

            (id, 0x01) if id % 2 == 0 => {
                // Start of property: write opening {
                if !first_property {
                    json.push(b',');
                }
                first_property = false;
                json.extend_from_slice(br#"{"#);

                // Write "property_id":"..."
                json.extend_from_slice(br#""property_id":"#);
                if write_quoted_property_id(&mut json, field_data).is_err() {
                    return invalid_response();
                }
                json.extend_from_slice(br#","days":["#);

                // Peek at next field: must be days vector (type 0x08)
                if offset + 7 > data.len() {
                    return invalid_response();
                }
                let next_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
                if next_id != current_field + 1 {
                    return invalid_response();
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
                    return invalid_response();
                }

                let days_data = &data[offset..offset + days_len];
                offset += days_len;

                if days_len < 2 {
                    return invalid_response();
                }

                let days_count = u16::from_le_bytes([days_data[0], days_data[1]]);
                let expected_days_len = 2 + 11 * days_count as usize;
                if days_len != expected_days_len {
                    return invalid_response();
                }

                let mut first_day = true;
                let mut d_off = 2;

                for _ in 0..days_count {
                    if !first_day {
                        json.push(b',');
                    }
                    first_day = false;

                    json.push(b'{');

                    // "date":"YYYY-MM-DD"
                    json.extend_from_slice(br#""date":""#);
                    let date_packed = u16::from_le_bytes([days_data[d_off], days_data[d_off + 1]]);
                    if write_packed_date(&mut json, date_packed).is_err() {
                        return invalid_response();
                    }

                    // "availability":X
                    let availability = days_data[d_off + 2];
                    json.extend_from_slice(br#"","availability":"#);
                    json.extend_from_slice(itoa::Buffer::new().format(availability).as_bytes());

                    // "final_price":XXXX
                    let final_price = u32::from_le_bytes([
                        days_data[d_off + 3],
                        days_data[d_off + 4],
                        days_data[d_off + 5],
                        days_data[d_off + 6],
                    ]);
                    json.extend_from_slice(br#","final_price":"#);
                    json.extend_from_slice(itoa::Buffer::new().format(final_price).as_bytes());

                    // "rate_feature":[...]
                    let rate_mask = u32::from_le_bytes([
                        days_data[d_off + 7],
                        days_data[d_off + 8],
                        days_data[d_off + 9],
                        days_data[d_off + 10],
                    ]);

                    json.push(b',');
                    write_rate_features_array(&mut json, rate_mask);

                    json.push(b'}'); // close day object

                    d_off += 11;
                }

                json.extend_from_slice(br#"]}"#); // close days[] and property {}
                current_field += 1; // skip over the days field we just consumed
            }

            _ => return invalid_response(),
        }

        current_field += 1;
    }

    // Final validation: all fields consumed
    if (current_field - 1) != total_fields {
        return invalid_response();
    }

    // Close JSON
    json.extend_from_slice(br#"]}"#); // close properties[] and root {}

    // eprintln!("DEBUG JSON: {}", String::from_utf8_lossy(&json));
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        json,
    )
        .into_response()
}
