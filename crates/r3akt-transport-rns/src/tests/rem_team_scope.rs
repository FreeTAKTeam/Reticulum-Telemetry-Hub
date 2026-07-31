use super::*;

#[test]
fn reticulumd_rpc_adapter_maps_field_commands_mission_envelope() {
    let mut rpc = RecordingReticulumdRpc::with_responses(vec![serde_json::json!({
        "messages": [{
            "id": "directory-command-1",
            "source": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "destination": "local-destination",
            "fields": {
                "11": "43341e5c822d99857fa6e8641f2ca9c0",
                "9": [{
                    "command_id": "hub-directory-123",
                    "command_type": "rem.registry.team_peers.list",
                    "source": {
                        "rns_identity": "11111111111111111111111111111111"
                    },
                    "timestamp": "2026-07-16T12:00:00Z",
                    "args": {}
                }]
            }
        }]
    })]);
    let mut adapter = ReticulumdRpcLxmfRsAdapter::new(
        "local-destination",
        ReticulumdRpcTransport::from_rpc(&mut rpc),
    );

    let frame = crate::test_block_on(adapter.receive_frame())
        .expect("receive")
        .expect("frame");
    let envelope = ProtocolEnvelope::decode_msgpack(&frame.bytes).expect("envelope");

    assert_eq!(
        envelope.source.to_string(),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(envelope.stable_dedupe_key(), "directory-command-1");
    let Payload::Command(command) = envelope.payload else {
        panic!("expected command");
    };
    assert_eq!(command.name, "rem.registry.team_peers.list");
    assert_eq!(command.correlation_id.as_deref(), Some("hub-directory-123"));
    assert_eq!(
        command.args,
        serde_json::json!({
            "_rem_team_uid": "43341e5c822d99857fa6e8641f2ca9c0"
        })
    );
}
