use r3akt_protocol::Payload;
use r3akt_transport_rns::{
    TransportError, reticulumd_event_to_envelope, reticulumd_message_to_envelope,
};
use serde_json::{Value, json};

fn message(fields: &Value) -> Value {
    json!({
        "id": "field-command-test-1",
        "source": "peer-field-command",
        "destination": "local-destination",
        "content": "",
        "fields": fields,
    })
}

fn command_from_message(fields: &Value) -> r3akt_protocol::Command {
    let envelope = reticulumd_message_to_envelope(&message(fields), "local-destination")
        .expect("decode message")
        .expect("envelope");
    let Payload::Command(command) = envelope.payload else {
        panic!("expected command payload");
    };
    command
}

#[test]
fn legacy_command_selector_decodes_join() {
    let command = command_from_message(&json!({"9": [{"Command": "join"}]}));

    assert_eq!(command.name, "join");
    assert_eq!(command.args, json!({}));
    assert_eq!(command.correlation_id, None);
}

#[test]
fn numeric_legacy_selector_decodes_leave() {
    let command = command_from_message(&json!({"9": [{"0": "leave"}]}));

    assert_eq!(command.name, "leave");
    assert_eq!(command.args, json!({}));
}

#[test]
fn only_first_field_command_entry_is_used() {
    let command = command_from_message(&json!({
        "9": [{"Command": "join"}, {"Command": "leave"}]
    }));

    assert_eq!(command.name, "join");
}

#[test]
fn legacy_named_fields_become_command_arguments() {
    let command = command_from_message(&json!({
        "9": [{
            "Command": "CreateTopic",
            "command_id": "legacy-topic-1",
            "TopicID": "ops",
            "TopicName": "Operations",
            "TopicPath": "ops",
        }]
    }));

    assert_eq!(command.name, "CreateTopic");
    assert_eq!(command.correlation_id.as_deref(), Some("legacy-topic-1"));
    assert_eq!(
        command.args,
        json!({
            "TopicID": "ops",
            "TopicName": "Operations",
            "TopicPath": "ops",
        })
    );
}

#[test]
fn mission_command_envelope_remains_unchanged() {
    let command = command_from_message(&json!({
        "9": [{
            "command_id": "mission-1",
            "command_type": "mission.registry.team_peers.list",
            "source": {"rns_identity": "peer-field-command"},
            "timestamp": "2026-08-02T12:00:00Z",
            "args": {"team_uid": "team-1"},
        }]
    }));

    assert_eq!(command.name, "mission.registry.team_peers.list");
    assert_eq!(command.correlation_id.as_deref(), Some("mission-1"));
    assert_eq!(command.args, json!({"team_uid": "team-1"}));
}

#[test]
fn malformed_field_commands_are_reported_with_context() {
    let error = reticulumd_message_to_envelope(
        &message(&json!({"9": [{"not_a_selector": "join"}]})),
        "local-destination",
    )
    .expect_err("malformed field commands should not become chat");

    let TransportError::Receive(error) = error else {
        panic!("expected receive error");
    };
    assert!(error.contains("FIELD_COMMANDS (0x09)"));
    assert!(error.contains("field-command-test-1"));
    assert!(error.contains("peer-field-command"));
}

#[test]
fn ordinary_messages_without_commands_remain_chat() {
    let mut ordinary = message(&json!({"content_type": "text/plain"}));
    ordinary["content"] = Value::String("ordinary chat".to_string());
    let envelope = reticulumd_message_to_envelope(&ordinary, "local-destination")
        .expect("decode message")
        .expect("chat envelope");

    let Payload::TopicMessage(message) = envelope.payload else {
        panic!("expected topic message");
    };
    assert_eq!(message.body, "ordinary chat");
}

#[test]
fn event_message_uses_the_same_field_command_decoder() {
    let event = r3akt_transport_rns::ReticulumdEventRecord {
        event_id: "event-field-command-1".to_string(),
        runtime_id: None,
        stream_id: None,
        seq_no: None,
        contract_version: None,
        ts_ms: None,
        event_type: "inbound".to_string(),
        severity: None,
        source_component: None,
        operation_id: None,
        message_id: None,
        peer_id: None,
        correlation_id: None,
        trace_id: None,
        payload: json!({"message": message(&json!({"9": [{"Command": "join"}]}))}),
    };

    let envelope = reticulumd_event_to_envelope(&event, "local-destination")
        .expect("decode event")
        .expect("command envelope");
    assert!(matches!(envelope.payload, Payload::Command(command) if command.name == "join"));
}
