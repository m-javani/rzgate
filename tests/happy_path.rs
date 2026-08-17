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
