// // SPDX-License-Identifier: BUSL-1.1
// // Copyright (c) 2026 M. Javani
// //
// // This file is part of rzproxy.
// //
// // Use of this software is governed by the Business Source License 1.1
// // included in the LICENSE file in the root of this repository.

use crate::common::helper::TestHelper;

mod common;

#[tokio::test]
async fn test_get_prop_room_day_http() {
    let helper = TestHelper::new().await;

    let date = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();

    let result = helper.get_prop_room_day("s1_seg1_p1", "room1", &date).await;

    assert!(result.is_ok(), "Command should succeed: {:?}", result);

    let json = result.unwrap();
    assert_eq!(json["status"], "success");
    assert_eq!(json["property_id"], "s1_seg1_p1");
    assert_eq!(json["date"], date);

    let availability = json["availability"].as_u64().unwrap();
    assert!(availability >= 1);

    // Rate features should be an array
    assert!(json["rate_feature"].is_array());
}
