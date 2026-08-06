#[test]
fn snapshot_rejects_duplicate_normalized_subscriptions_and_preserves_compatibility_rows() {
    let mut core = RchCore::new();
    core.handle_command(&command(
        "topic.create",
        json!({ "topic_id": "mission-1", "topic_path": "mission-1", "topic_name": "Mission 1" }),
    ));
    core.handle_command(&command(
        "topic.subscribe",
        json!({ "topic_id": "mission-1", "destination": "FACEFEED" }),
    ));

    let mut duplicate = core.snapshot();
    duplicate.subscribers.push(SubscriberRecord {
        node_id: " FACEFEED ".to_string(),
        topic_id: " mission-1 ".to_string(),
        first_seen_ts_ms: 0,
        last_seen_ts_ms: 0,
        reject_tests: None,
        metadata: json!({}),
    });
    let duplicate_error = RchCore::from_snapshot(duplicate).expect_err("duplicate rejected");
    assert!(
        duplicate_error
            .to_string()
            .contains("duplicate normalized destination/topic pair")
    );

    let mut orphan = core.snapshot();
    orphan.subscribers[0].topic_id = "missing-topic".to_string();
    let restored_orphan = RchCore::from_snapshot(orphan).expect("orphan preserved");
    assert_eq!(
        restored_orphan.snapshot().subscribers[0].topic_id,
        "missing-topic"
    );

    let mut topicless = core.snapshot();
    topicless.subscribers[0].topic_id = " ".to_string();
    let restored_topicless = RchCore::from_snapshot(topicless).expect("topicless preserved");
    assert_eq!(restored_topicless.snapshot().subscribers[0].topic_id, "");
}

#[test]
fn sqlite_read_snapshot_keeps_topicless_compatibility_subscribers_loadable() {
    let db_path = std::env::temp_dir().join(format!(
        "r3akt-rch-core-topicless-subscriber-{}.db",
        Uuid::new_v4()
    ));
    let mut store = RchSqliteStore::open(&db_path).expect("sqlite");
    store
        .upsert_topic(&TopicRecord {
            topic_id: "ops".to_string(),
            topic_name: "Ops".to_string(),
            topic_path: "ops".to_string(),
            topic_description: String::new(),
            retention: RetentionPolicy::Persistent,
            visibility: Visibility::Public,
            created_ts_ms: 1,
            last_activity_ts_ms: 1,
        })
        .expect("topic");
    store
        .upsert_subscriber(&SubscriberRecord {
            node_id: "DEST-COMPAT".to_string(),
            topic_id: String::new(),
            first_seen_ts_ms: 1,
            last_seen_ts_ms: 1,
            reject_tests: None,
            metadata: json!({ "source": "compatibility" }),
        })
        .expect("subscriber");

    let snapshot = store
        .load_r3akt_read_snapshot()
        .expect("read snapshot");
    let core = RchCore::from_snapshot(snapshot).expect("compatibility snapshot");
    assert_eq!(core.snapshot().subscribers[0].node_id, "DEST-COMPAT");
    assert_eq!(core.snapshot().subscribers[0].topic_id, "");

    drop(store);
    let _ = std::fs::remove_file(db_path);
}
#[test]
fn sqlite_migration_is_additive_for_existing_database_like_python_startup() {
    let db_path = std::env::temp_dir().join(format!(
        "r3akt-rch-core-additive-migration-{}.db",
        Uuid::new_v4()
    ));
    {
        let connection = Connection::open(&db_path).expect("sqlite");
        connection
            .execute_batch(
                "CREATE TABLE legacy_probe (id TEXT PRIMARY KEY);
                 INSERT INTO legacy_probe (id) VALUES ('probe-1');",
            )
            .expect("legacy table");
    }

    let store = RchSqliteStore::open(&db_path).expect("migrated store");
    assert_eq!(store.schema_version().expect("schema version"), "3");
    let migration_count: i64 = store
        .connection
        .query_row("SELECT COUNT(*) FROM rch_schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("migration count");
    assert_eq!(migration_count, 3);
    let index_count: i64 = store
        .connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_rch_subscribers_topic_node'",
            [],
            |row| row.get(0),
        )
        .expect("subscriber topic index");
    assert_eq!(index_count, 1);
    drop(store);

    let reopened = RchSqliteStore::open(&db_path).expect("reopened store");
    assert_eq!(reopened.schema_version().expect("schema version"), "3");
    let reopened_migration_count: i64 = reopened
        .connection
        .query_row("SELECT COUNT(*) FROM rch_schema_migrations", [], |row| {
            row.get(0)
        })
        .expect("reopened migration count");
    assert_eq!(reopened_migration_count, 3);
    drop(reopened);

    let connection = Connection::open(&db_path).expect("sqlite");
    let legacy_id: String = connection
        .query_row(
            "SELECT id FROM legacy_probe WHERE id = 'probe-1'",
            [],
            |row| row.get(0),
        )
        .expect("legacy row");
    let rch_topic_table: String = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'rch_topics'",
            [],
            |row| row.get(0),
        )
        .expect("rch table");

    assert_eq!(legacy_id, "probe-1");
    assert_eq!(rch_topic_table, "rch_topics");
    drop(connection);
    let _ = std::fs::remove_file(db_path);
}

#[test]
fn sqlite_migration_preserves_existing_subscriber_payloads() {
    let db_path = std::env::temp_dir().join(format!(
        "r3akt-rch-core-subscriber-migration-{}.db",
        Uuid::new_v4()
    ));
    let topic = TopicRecord {
        topic_id: "mission-legacy".to_string(),
        topic_name: "Legacy Mission".to_string(),
        topic_path: "mission-legacy".to_string(),
        topic_description: String::new(),
        retention: RetentionPolicy::Persistent,
        visibility: Visibility::Public,
        created_ts_ms: 1,
        last_activity_ts_ms: 2,
    };
    let subscriber = SubscriberRecord {
        node_id: "DEST-LEGACY".to_string(),
        topic_id: "mission-legacy".to_string(),
        first_seen_ts_ms: 3,
        last_seen_ts_ms: 4,
        reject_tests: Some(1),
        metadata: json!({ "source": "legacy" }),
    };
    {
        let connection = Connection::open(&db_path).expect("sqlite");
        connection
            .execute_batch(include_str!("../migrations/0001_rch_core_snapshot.sql"))
            .expect("v1 migration");
        connection
            .execute_batch(include_str!("../migrations/0002_ordered_migrations.sql"))
            .expect("v2 migration");
        connection
            .execute(
                "INSERT INTO rch_schema_migrations (version, name, applied_ts_ms) VALUES (1, 'rch_core_snapshot', 1), (2, 'ordered_migrations', 2)",
                [],
            )
            .expect("migration history");
        connection
            .execute(
                "INSERT INTO rch_topics (topic_id, payload) VALUES (?1, ?2)",
                params![
                    &topic.topic_id,
                    encode_msgpack(&topic).expect("topic payload")
                ],
            )
            .expect("topic payload");
        connection
            .execute(
                "INSERT INTO rch_subscribers (node_id, topic_id, payload) VALUES (?1, ?2, ?3)",
                params![
                    &subscriber.node_id,
                    &subscriber.topic_id,
                    encode_msgpack(&subscriber).expect("subscriber payload")
                ],
            )
            .expect("subscriber payload");
    }

    let store = RchSqliteStore::open(&db_path).expect("migrated store");
    assert_eq!(store.schema_version().expect("schema version"), "3");
    let snapshot = store
        .load_snapshot()
        .expect("load snapshot")
        .expect("snapshot");
    assert_eq!(snapshot.topics, vec![topic]);
    assert_eq!(snapshot.subscribers, vec![subscriber]);
    drop(store);
    let _ = std::fs::remove_file(db_path);
}
