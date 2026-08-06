#[test]
fn runtime_diagnostics_reports_topic_subscription_invariants() {
    let state = crate::AppState::default();
    state.topics.write().expect("topics").insert(
        "ops".to_string(),
        crate::TopicRecord {
            topic_id: "ops".to_string(),
            topic_name: "Ops".to_string(),
            topic_path: "ops".to_string(),
            topic_description: String::new(),
        },
    );
    let mut subscribers = state.subscribers.write().expect("subscribers");
    subscribers.insert(
        "one".to_string(),
        crate::SubscriberRecord {
            subscriber_id: "one".to_string(),
            destination: "AABB".to_string(),
            topic_id: "ops".to_string(),
            reject_tests: None,
            metadata: json!({}),
        },
    );
    subscribers.insert(
        "two".to_string(),
        crate::SubscriberRecord {
            subscriber_id: "two".to_string(),
            destination: " aabb ".to_string(),
            topic_id: "ops".to_string(),
            reject_tests: None,
            metadata: json!({}),
        },
    );
    subscribers.insert(
        "three".to_string(),
        crate::SubscriberRecord {
            subscriber_id: "three".to_string(),
            destination: String::new(),
            topic_id: "missing".to_string(),
            reject_tests: None,
            metadata: json!({}),
        },
    );
    drop(subscribers);

    let diagnostics = crate::runtime_diagnostics_payload(&state).expect("diagnostics");
    assert_eq!(diagnostics["topic_count"], 1);
    assert_eq!(diagnostics["subscriber_count"], 3);
    assert_eq!(diagnostics["orphan_subscriber_count"], 1);
    assert_eq!(diagnostics["duplicate_normalized_subscription_count"], 1);
    assert_eq!(diagnostics["missing_subscriber_identity_count"], 3);
    assert_eq!(diagnostics["topics"]["count"], 1);

    crate::topic_diagnostics::record_identity_announce_update_error(
        &state,
        Some("identity update failed".to_string()),
    )
    .expect("record identity error");
    let diagnostics = crate::runtime_diagnostics_payload(&state).expect("diagnostics");
    assert_eq!(
        diagnostics["last_identity_announce_update_error"],
        "identity update failed"
    );
}
