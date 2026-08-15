// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzgate.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use axum::response::Response;

use crate::{
    handler::handler::Handler,
    metrics::MetricsRef,
    processor::{
        dec_room_avl::process_dec_room_avl, del_prop::process_del_prop,
        del_prop_day::process_del_prop_day, del_prop_room::process_del_prop_room,
        del_room_day::process_del_room_day, del_segment::process_del_segment,
        get_prop_room_day::process_get_prop_room_day, get_segments::process_get_segments,
        inc_room_avl::process_inc_room_avl, prop_exist::process_prop_exist,
        prop_room_date_list::process_prop_room_date_list, prop_room_exist::process_prop_room_exist,
        prop_room_list::process_prop_room_list, search_avail::process_search_avail,
        search_prop::process_search_prop, set_prop::process_set_prop,
        set_room_avl::process_set_room_avl, set_room_pkg::process_set_room_pkg,
    },
    protocol::error_response,
};

use serde_json::{Value, from_slice};

pub async fn process(body: &[u8], handler: &Handler, metrics: MetricsRef) -> Response {
    metrics.inc_commands();
    metrics.add_bytes_received(body.len() as u64);

    // Parse only once, minimal overhead for 150B
    let json: Value = match from_slice(body) {
        Ok(v) => v,
        Err(_) => return error_response(metrics, "Invalid JSON").await,
    };

    // Get command with zero-copy reference
    let command = match json.get("command").and_then(|v| v.as_str()) {
        Some(cmd) => cmd,
        None => return error_response(metrics, "Missing command field").await,
    };

    let segment = match json.get("segment").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return error_response(metrics, "Missing segment field").await;
        }
    };

    // Get body reference (doesn't copy data)
    let payload = json.get("body").unwrap_or(&Value::Null);

    match command.as_ref() {
        "SETPROP" => process_set_prop(segment, payload, handler, metrics).await,
        "PROPEXIST" => process_prop_exist(segment, payload, handler, metrics).await,
        "SEARCHPROP" => process_search_prop(segment, payload, handler, metrics).await,

        "SETROOMPKG" => process_set_room_pkg(segment, payload, handler, metrics).await,
        "SETROOMAVL" => process_set_room_avl(segment, payload, handler, metrics).await,
        "INCROOMAVL" => process_inc_room_avl(segment, payload, handler, metrics).await,
        "DECROOMAVL" => process_dec_room_avl(segment, payload, handler, metrics).await,
        "DELROOMDAY" => process_del_room_day(segment, payload, handler, metrics).await,
        "PROPROOMEXIST" => process_prop_room_exist(segment, payload, handler, metrics).await,
        "GETPROPROOMDAY" => process_get_prop_room_day(segment, payload, handler, metrics).await,
        "PROPROOMDATELIST" => process_prop_room_date_list(segment, payload, handler, metrics).await,
        "DELPROPROOM" => process_del_prop_room(segment, payload, handler, metrics).await,

        "SEARCHAVAIL" => process_search_avail(segment, payload, handler, metrics).await,

        "PROPROOMLIST" => process_prop_room_list(segment, payload, handler, metrics).await,
        "DELPROP" => process_del_prop(segment, payload, handler, metrics).await,
        "DELSEGMENT" => process_del_segment(segment, payload, handler, metrics).await,
        "DELPROPDAY" => process_del_prop_day(segment, payload, handler, metrics).await,
        "GETSEGMENTS" => process_get_segments(payload, handler, metrics).await,

        _ => error_response(metrics, "unsupported command").await,
    }
}
