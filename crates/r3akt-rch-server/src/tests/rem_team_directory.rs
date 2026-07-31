use super::*;

#[tokio::test]
async fn reticulumd_inbound_team_peer_directory_returns_only_shared_team_rem_peers() {
    const YELLOW_TEAM_UID: &str = "d6b6e188b910d6bdd24d04b7a7ec5444";
    const BLUE_TEAM_UID: &str = "43341e5c822d99857fa6e8641f2ca9c0";
    const CALLER_IDENTITY: &str = "11111111111111111111111111111111";
    const CALLER_DESTINATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TEAMMATE_IDENTITY: &str = "22222222222222222222222222222222";
    const TEAMMATE_DESTINATION: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const OUTSIDER_IDENTITY: &str = "33333333333333333333333333333333";

    let setup_command = |command_type: &str, args: Value| {
        let command_id = format!("setup-{command_type}-{args}");
        r3akt_profile_rch::MissionCommandEnvelope {
            command_id,
            source: r3akt_profile_rch::RchSource::new("setup-source"),
            timestamp: "2026-07-16T12:00:00Z".to_string(),
            command_type: command_type.to_string(),
            args,
            correlation_id: None,
            topics: Vec::new(),
        }
    };
    let mut core = RchCore::new();
    for (uid, name) in [(YELLOW_TEAM_UID, "Yellow"), (BLUE_TEAM_UID, "Blue")] {
        core.handle_command(&setup_command(
            "mission.registry.team.upsert",
            json!({ "uid": uid, "team_name": name }),
        ));
    }
    for (uid, team_uid, identity) in [
        ("member-caller", YELLOW_TEAM_UID, CALLER_IDENTITY),
        ("member-teammate", YELLOW_TEAM_UID, TEAMMATE_IDENTITY),
        ("member-outsider", BLUE_TEAM_UID, OUTSIDER_IDENTITY),
    ] {
        core.handle_command(&setup_command(
            "mission.registry.team_member.upsert",
            json!({
                "uid": uid,
                "team_uid": team_uid,
                "rns_identity": identity,
                "display_name": uid,
            }),
        ));
    }
    let rem_capabilities = vec![
        "r3akt".to_string(),
        "EmergencyMessages".to_string(),
        "Telemetry".to_string(),
    ];
    for (identity, destination, display_name) in [
        (CALLER_IDENTITY, CALLER_DESTINATION, "Caller"),
        (TEAMMATE_IDENTITY, TEAMMATE_DESTINATION, "Teammate"),
    ] {
        core.record_identity_announce(
            destination,
            Some(identity.to_string()),
            Some(display_name.to_string()),
            Some("destination".to_string()),
            rem_capabilities.clone(),
        )
        .expect("REM destination announce");
    }
    core.record_identity_announce(
        OUTSIDER_IDENTITY,
        None,
        Some("Outsider".to_string()),
        Some("identity".to_string()),
        rem_capabilities,
    )
    .expect("outsider announce");

    let db_path = std::env::temp_dir().join(format!(
        "r3akt-reticulumd-team-peer-directory-{}.db",
        Uuid::new_v4()
    ));
    {
        let mut store = RchSqliteStore::open(&db_path).expect("store");
        store.save_snapshot(&core.snapshot()).expect("snapshot");
    }
    let (endpoint, rpc_server) = fake_reticulumd_rpc_server_with_results_and_accept_timeout(
        vec![
            json!({
                "messages": [{
                    "id": "lxmf-team-directory-1",
                    "source": CALLER_DESTINATION,
                    "destination": "local-destination",
                    "fields": { crate::FIELD_COMMANDS.to_string(): [{
                        "command_id": "hub-directory-123",
                        "command_type": "rem.registry.team_peers.list",
                        "source": { "rns_identity": CALLER_IDENTITY },
                        "timestamp": "2026-07-16T12:00:00Z",
                        "args": {}
                    }] }
                }]
            }),
            json!({"message_id": "accepted-reply"}),
            json!({"message_id": "result-reply"}),
        ],
        Duration::from_secs(10),
    );
    let state = crate::AppState::from_sqlite_path(&db_path)
        .expect("state")
        .with_reticulumd_rpc(endpoint.clone(), "local-destination");
    let adapter =
        ReticulumdRpcLxmfRsAdapter::new("local-destination", ReticulumdRpcTransport::new(endpoint));
    let mut transport = LxmfRsTransport::new(adapter);

    crate::process_reticulumd_inbound_worker_tick(
        &state,
        &mut transport,
        Duration::from_millis(50),
    )
    .await
    .expect("inbound tick")
    .expect("envelope");

    let requests = rpc_server.join().expect("rpc server");
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "list_messages");
    let replies = requests
        .iter()
        .skip(1)
        .map(|request| request.params.as_ref().expect("params"))
        .collect::<Vec<_>>();
    assert!(replies.iter().any(|params| {
        params["fields"]["10"]["status"] == "accepted"
            && params["fields"]["10"]["command_id"] == "hub-directory-123"
    }));
    let result = replies
        .iter()
        .find(|params| params["fields"]["10"]["status"] == "result")
        .expect("result reply");
    assert_eq!(result["fields"]["10"]["command_id"], "hub-directory-123");
    assert_eq!(result["fields"]["10"]["result"]["scope"], "shared_teams");
    assert_eq!(result["fields"]["10"]["result"]["schema_version"], 2);
    assert_eq!(
        result["fields"]["10"]["result"]["caller_memberships"][0]["team_uid"],
        YELLOW_TEAM_UID
    );
    assert_eq!(
        result["fields"]["10"]["result"]["members"][0]["team_uid"],
        YELLOW_TEAM_UID
    );
    let items = result["fields"]["10"]["result"]["items"]
        .as_array()
        .expect("items");
    assert_eq!(items.len(), 1, "result reply: {result:#?}");
    assert_eq!(items[0]["identity"], TEAMMATE_IDENTITY);
    assert_eq!(items[0]["destination_hash"], TEAMMATE_DESTINATION);

    std::fs::remove_file(db_path).expect("cleanup db");
}
