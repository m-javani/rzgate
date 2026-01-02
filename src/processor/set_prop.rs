// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::metrics::MetricsEvent;
use crate::processor::base::decode_simple_response;
use crate::{handler::handler::Handler, processor::base::error_response};
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc::Sender;

pub async fn process_set_prop(
    payload: &Value,
    handler: &Handler,
    metrics_tx: Sender<MetricsEvent>,
) -> Response {
    // extract, validate and build binary payload from fields
    let segment = match payload.get("segment").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return error_response(metrics_tx.clone(), "segment is required").await,
    };

    let area = match payload.get("area").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return error_response(metrics_tx.clone(), "area is required").await,
    };

    let property_id = match payload.get("property_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response(metrics_tx.clone(), "property_id is required").await,
    };

    let property_type = match payload.get("property_type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return error_response(metrics_tx.clone(), "property_type is required").await,
    };

    let category = match payload.get("category").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return error_response(metrics_tx.clone(), "category is required").await,
    };

    let stars = match payload.get("stars").and_then(|v| v.as_u64()) {
        Some(s) if s >= 1 && s <= 5 => s as u8,
        _ => return error_response(metrics_tx.clone(), "stars must be between 1 and 5").await,
    };

    let latitude = match payload.get("latitude").and_then(|v| v.as_f64()) {
        Some(lat) if lat >= -90.0 && lat <= 90.0 => lat,
        _ => {
            return error_response(metrics_tx.clone(), "latitude must be between -90 and 90").await;
        }
    };

    let longitude = match payload.get("longitude").and_then(|v| v.as_f64()) {
        Some(lon) if lon >= -180.0 && lon <= 180.0 => lon,
        _ => {
            return error_response(metrics_tx.clone(), "longitude must be between -180 and 180")
                .await;
        }
    };

    let amenities = match payload.get("amenities").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str()).collect::<Vec<&str>>(),
        None => Vec::new(),
    };

    // Build payload inline
    let mut buf = Vec::new();

    // Command name
    let cmd_name = "SETPROP";
    buf.push(cmd_name.len() as u8);
    buf.extend_from_slice(cmd_name.as_bytes());

    // Amenities as comma-separated string
    let amenity_str = amenities.join(",");

    // Field count placeholder (we'll patch this later)
    let field_count_pos = buf.len();
    buf.extend_from_slice(&0u16.to_le_bytes()); // placeholder for field count

    // Helper to add a field
    let mut field_count = 0u16;
    let mut add_field = |id: u16, typ: u8, data: &[u8]| {
        buf.extend_from_slice(&id.to_le_bytes());
        buf.push(typ);
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(data);
        field_count += 1;
    };

    // Add all fields
    add_field(0x01, 0x01, segment.as_bytes()); // Segment
    add_field(0x02, 0x01, area.as_bytes()); // Area
    add_field(0x03, 0x01, property_id.as_bytes()); // PropertyID
    add_field(0x04, 0x01, property_type.as_bytes()); // PropertyType
    add_field(0x05, 0x01, category.as_bytes()); // Category
    add_field(0x06, 0x02, &[stars]); // Stars
    add_field(0x07, 0x03, &latitude.to_le_bytes()); // Latitude
    add_field(0x08, 0x03, &longitude.to_le_bytes()); // Longitude
    add_field(0x09, 0x01, amenity_str.as_bytes()); // Amenities

    // Patch field count
    let field_count_bytes = field_count.to_le_bytes();
    buf[field_count_pos] = field_count_bytes[0];
    buf[field_count_pos + 1] = field_count_bytes[1];

    match handler.execute(true, buf).await {
        Ok(field_data) => decode_set_prop_response(metrics_tx.clone(), &field_data),
        Err(e) => error_response(metrics_tx.clone(), &e.to_string()).await,
    }
}

fn decode_set_prop_response(metrics_tx: Sender<MetricsEvent>, payload: &Bytes) -> Response {
    decode_simple_response(metrics_tx.clone(), payload)
}
