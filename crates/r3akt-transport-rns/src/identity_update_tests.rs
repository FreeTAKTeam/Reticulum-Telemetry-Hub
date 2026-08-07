#[test]
fn zmq_data_plane_registers_and_updates_independent_rch_identity() {
    let (command_endpoint, response_endpoint) = unused_zmq_endpoint_pair_v4();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let identity_result = serde_json::json!({
        "identity": {
            "identity": "11111111111111111111111111111111",
            "delivery_destination": "22222222222222222222222222222222",
            "public_key": "public-key",
            "display_name": "RCH",
            "capabilities": ["r3akt"],
            "metadata": {"service": "rch"},
            "extensions": {}
        }
    });
    let server = spawn_zmq_sequence_server(
        command_endpoint.clone(),
        vec![
            serde_json::json!({
                "runtime_id": "runtime-rch-zmq",
                "effective_capabilities": [
                    "sdk.capability.identity_multi",
                    "sdk.capability.identity_import_export",
                    "sdk.capability.identity_discovery"
                ]
            }),
            identity_result.clone(),
            serde_json::json!({"accepted": true}),
            serde_json::json!({
                "accepted": true,
                "identity": "11111111111111111111111111111111",
                "delivery_destination": "22222222222222222222222222222222"
            }),
            identity_result,
            serde_json::json!({"accepted": true}),
            serde_json::json!({
                "accepted": true,
                "identity": "11111111111111111111111111111111",
                "delivery_destination": "22222222222222222222222222222222"
            }),
        ],
        Arc::clone(&captured),
    );
    let data_plane =
        ZmqDataPlane::new(command_endpoint, response_endpoint).expect("data plane");
    let registered = data_plane
        .register_identity(RchServiceIdentityConfig {
            private_key: vec![7_u8; 64],
            display_name: "RCH".to_string(),
            capabilities: vec!["r3akt".to_string()],
            metadata: BTreeMap::from([("service".to_string(), serde_json::json!("rch"))]),
        })
        .expect("register identity");
    let updated = data_plane
        .update_identity_announce(
            "Field RCH",
            vec!["r3akt".to_string(), "telemetry".to_string()],
            BTreeMap::from([
                ("service".to_string(), serde_json::json!("rch")),
                ("revision".to_string(), serde_json::json!(2)),
            ]),
        )
        .expect("update identity");
    data_plane.shutdown().expect("shutdown");
    server.join().expect("server joined");

    assert_eq!(registered.identity.0, updated.identity.0);
    assert_eq!(
        registered.delivery_destination,
        updated.delivery_destination
    );
    let captured = captured.lock().expect("captured requests");
    assert_eq!(
        captured
            .iter()
            .map(|request| request.method.as_str())
            .collect::<Vec<_>>(),
        vec![
            "sdk_negotiate_v2",
            "sdk_identity_import_v2",
            "sdk_identity_activate_v2",
            "sdk_identity_announce_now_v2",
            "sdk_identity_import_v2",
            "sdk_identity_activate_v2",
            "sdk_identity_announce_now_v2",
        ]
    );
    assert_eq!(captured[1].params["display_name"], "RCH");
    assert_eq!(captured[3].params["display_name"], "RCH");
    assert_eq!(captured[4].params["display_name"], "Field RCH");
    assert_eq!(captured[6].params["display_name"], "Field RCH");
    assert_eq!(
        captured[6].params["capabilities"],
        serde_json::json!(["r3akt", "telemetry"])
    );
    assert_eq!(captured[4].params["metadata"]["service"], "rch");
    assert_eq!(captured[4].params["metadata"]["revision"], 2);
    assert_eq!(captured[6].params["metadata"]["revision"], 2);
}

#[test]
fn zmq_data_plane_identity_update_maps_sdk_errors() {
    let (command_endpoint, response_endpoint) = unused_zmq_endpoint_pair_v4();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let identity_result = serde_json::json!({
        "identity": {
            "identity": "11111111111111111111111111111111",
            "delivery_destination": "22222222222222222222222222222222",
            "public_key": "public-key",
            "display_name": "RCH",
            "capabilities": ["r3akt"],
            "metadata": {"service": "rch"},
            "extensions": {}
        }
    });
    let server = spawn_zmq_sequence_server(
        command_endpoint.clone(),
        vec![
            serde_json::json!({"runtime_id": "runtime-rch-zmq"}),
            identity_result,
            serde_json::json!({"accepted": true}),
            serde_json::json!({"accepted": true}),
            serde_json::json!({
                "__rpc_error": {
                    "code": "SDK_IDENTITY_ANNOUNCE_FAILED",
                    "message": "identity import rejected",
                    "machine_code": "SDK_IDENTITY_ANNOUNCE_FAILED",
                    "category": "identity",
                    "retryable": false
                }
            }),
        ],
        Arc::clone(&captured),
    );
    let data_plane =
        ZmqDataPlane::new(command_endpoint, response_endpoint).expect("data plane");
    data_plane
        .register_identity(RchServiceIdentityConfig {
            private_key: vec![7_u8; 64],
            display_name: "RCH".to_string(),
            capabilities: vec!["r3akt".to_string()],
            metadata: BTreeMap::new(),
        })
        .expect("register identity");
    let error = data_plane
        .update_identity_announce("Field RCH", vec!["r3akt".to_string()], BTreeMap::new())
        .expect_err("identity update error");
    data_plane.shutdown().expect("shutdown");
    drop(data_plane);
    server.join().expect("server joined");

    match error {
        TransportError::Sdk {
            code,
            category,
            retryable,
            message,
        } => {
            assert_eq!(code, "SDK_IDENTITY_ANNOUNCE_FAILED");
            assert_eq!(category.as_deref(), Some("Internal"));
            assert!(!retryable);
            assert_eq!(message, "identity import rejected");
        }
        other => panic!("expected SDK error, got {other:?}"),
    }
    let captured = captured.lock().expect("captured requests");
    assert_eq!(captured[4].method, "sdk_identity_import_v2");
}
