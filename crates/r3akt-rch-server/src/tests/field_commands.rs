use super::*;

use r3akt_rch_bridge::{ReticulumdRpc, ReticulumdRpcClient};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

fn inbound_event(
    event_id: &str,
    message_id: &str,
    source: &str,
    fields: Value,
    content: &str,
) -> r3akt_transport_rns::ReticulumdEventRecord {
    r3akt_transport_rns::ReticulumdEventRecord {
        event_id: event_id.to_string(),
        runtime_id: None,
        stream_id: None,
        seq_no: None,
        contract_version: None,
        ts_ms: None,
        event_type: "inbound".to_string(),
        severity: None,
        source_component: None,
        operation_id: None,
        message_id: Some(message_id.to_string()),
        peer_id: None,
        correlation_id: None,
        trace_id: None,
        payload: json!({
            "message": {
                "id": message_id,
                "source": source,
                "destination": "hub-source",
                "content": content,
                "fields": fields,
            }
        }),
    }
}

#[test]
fn command_docs_show_legacy_and_mission_field_shapes() {
    let help = crate::command_help_text();
    assert!(help.contains("FIELD_COMMANDS"));
    assert!(help.contains(r#"{"9":[{"Command":"join"}]}"#));
    assert!(help.contains(r#"{"9":[{"0":"join"}]}"#));

    let examples = crate::supported_commands_document();
    assert!(examples.contains(r#"{"9":[{"Command":"join"}]}"#));
    assert!(examples.contains(r#"{"9":[{"0":"leave"}]}"#));
    assert!(examples.contains("remaining fields in the first entry become command arguments"));
    assert!(examples.contains("command_type` plus `args`"));
}

#[test]
fn list_messages_legacy_join_adds_source_replies_and_deduplicates() {
    let (endpoint, rpc_server) = fake_reticulumd_rpc_server();
    let state = crate::AppState::default().with_reticulumd_rpc(endpoint, "hub-source");
    let source = "77b2539b72259af927e48c0f90721767";
    let result = json!({
        "messages": [{
            "id": "field-command-join-1",
            "direction": "in",
            "source": source,
            "destination": "hub-source",
            "content": "",
            "fields": {"9": [{"Command": "join"}]},
            "timestamp": 1_775_000_001,
            "title": ""
        }]
    });

    let first = crate::process_reticulumd_list_messages_result(&state, "hub-source", &result)
        .expect("first import");
    let second = crate::process_reticulumd_list_messages_result(&state, "hub-source", &result)
        .expect("duplicate import");

    assert_eq!(first, 1);
    assert_eq!(second, 0);
    assert!(state.clients.read().expect("clients").contains_key(source));
    let messages = state.messages.read().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "joined");
    drop(messages);

    let request = rpc_server.join().expect("rpc server");
    assert_eq!(request.method, "sdk_send_v2");
    let params = request.params.expect("params");
    assert_eq!(params["destination"], source);
    assert_eq!(params["content"], "joined");
}

#[test]
fn list_messages_numeric_legacy_leave_removes_source_and_replies() {
    let (endpoint, rpc_server) = fake_reticulumd_rpc_server();
    let state = crate::AppState::default().with_reticulumd_rpc(endpoint, "hub-source");
    let source = "88b2539b72259af927e48c0f90721767";
    crate::upsert_roster_client(&state, source, |_| {}).expect("seed roster");

    let imported = crate::process_reticulumd_list_messages_result(
        &state,
        "hub-source",
        &json!({
            "messages": [{
                "id": "field-command-leave-1",
                "direction": "inbound",
                "source": source,
                "destination": "hub-source",
                "content": "",
                "fields": {"9": [{"0": "leave"}]},
                "timestamp": 1_775_000_002,
                "title": ""
            }]
        }),
    )
    .expect("process list messages");

    assert_eq!(imported, 1);
    assert!(!state.clients.read().expect("clients").contains_key(source));
    let messages = state.messages.read().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "left");
    drop(messages);

    let request = rpc_server.join().expect("rpc server");
    assert_eq!(request.method, "sdk_send_v2");
    let params = request.params.expect("params");
    assert_eq!(params["destination"], source);
    assert_eq!(params["content"], "left");
}

#[test]
fn list_messages_mission_style_join_still_dispatches() {
    let (endpoint, rpc_server) = fake_reticulumd_rpc_server();
    let state = crate::AppState::default().with_reticulumd_rpc(endpoint, "hub-source");
    let source = "99b2539b72259af927e48c0f90721767";

    let imported = crate::process_reticulumd_list_messages_result(
        &state,
        "hub-source",
        &json!({
            "messages": [{
                "id": "field-command-mission-join-1",
                "direction": "in",
                "source": source,
                "destination": "hub-source",
                "content": "",
                "fields": {"9": [{
                    "command_id": "mission-join-1",
                    "command_type": "mission.join",
                    "args": {}
                }]},
                "timestamp": 1_775_000_003,
                "title": ""
            }]
        }),
    )
    .expect("process list messages");

    assert_eq!(imported, 1);
    assert!(state.clients.read().expect("clients").contains_key(source));
    let messages = state.messages.read().expect("messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "joined");
    drop(messages);

    let request = rpc_server.join().expect("rpc server");
    assert_eq!(request.method, "sdk_send_v2");
    let params = request.params.expect("params");
    assert_eq!(params["destination"], source);
    assert_eq!(params["content"], "joined");
}

#[test]
fn malformed_event_is_quarantined_and_cursor_advances_to_later_messages() {
    let db_path = std::env::temp_dir().join(format!(
        "r3akt-rch-malformed-field-command-event-{}.db",
        Uuid::new_v4()
    ));
    let state = crate::AppState::from_sqlite_path(&db_path).expect("state");
    let report = crate::process_reticulumd_event_batch(
        &state,
        "hub-source",
        r3akt_transport_rns::ReticulumdEventBatch {
            events: vec![
                inbound_event(
                    "quarantine-event-1",
                    "quarantine-message-1",
                    "peer-field-command",
                    json!({"9": [{"not_a_selector": "join"}]}),
                    "",
                ),
                inbound_event(
                    "after-quarantine-event-1",
                    "after-quarantine-message-1",
                    "peer-after-quarantine",
                    json!({}),
                    "after quarantine",
                ),
            ],
            next_cursor: Some("stream:2".to_string()),
            dropped_count: 0,
            snapshot_high_watermark_seq_no: None,
        },
    )
    .expect("process event batch");

    assert_eq!(report.next_cursor.as_deref(), Some("stream:2"));
    assert_eq!(
        crate::load_reticulumd_event_cursor(&state).as_deref(),
        Some("stream:2")
    );
    assert!(
        state
            .messages
            .read()
            .expect("messages")
            .iter()
            .any(|message| message.content == "after quarantine")
    );
    let diagnostics = crate::runtime_diagnostics_payload(&state).expect("diagnostics");
    assert_eq!(diagnostics["reticulumd_inbound"]["quarantined_total"], 1);
    assert!(
        diagnostics["reticulumd_inbound"]["last_quarantine_error"]
            .as_str()
            .is_some_and(|error| error.contains("quarantine-message-1"))
    );
    assert!(
        state
            .system_events
            .read()
            .expect("system events")
            .iter()
            .any(|event| event.event_type == "reticulumd_inbound_message_quarantined")
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn malformed_list_message_is_quarantined_and_not_retried() {
    let db_path = std::env::temp_dir().join(format!(
        "r3akt-rch-malformed-field-command-list-{}.db",
        Uuid::new_v4()
    ));
    let state = crate::AppState::from_sqlite_path(&db_path).expect("state");
    let result = json!({
        "messages": [
            {
                "id": "quarantine-list-1",
                "direction": "in",
                "source": "peer-field-command",
                "destination": "hub-source",
                "content": "",
                "fields": {"9": [{"not_a_selector": "join"}]}
            },
            {
                "id": "after-quarantine-list-1",
                "direction": "in",
                "source": "peer-after-quarantine",
                "destination": "hub-source",
                "content": "after quarantine",
                "fields": {}
            }
        ]
    });

    assert_eq!(
        crate::process_reticulumd_list_messages_result(&state, "hub-source", &result)
            .expect("first list import"),
        1
    );
    assert_eq!(
        crate::process_reticulumd_list_messages_result(&state, "hub-source", &result)
            .expect("second list import"),
        0
    );
    let diagnostics = crate::runtime_diagnostics_payload(&state).expect("diagnostics");
    assert_eq!(diagnostics["reticulumd_inbound"]["quarantined_total"], 1);
    assert_eq!(
        state
            .system_events
            .read()
            .expect("system events")
            .iter()
            .filter(|event| event.event_type == "reticulumd_inbound_message_quarantined")
            .count(),
        1
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn live_reticulumd_field_command_join_reaches_rch_and_reply_is_delivered_when_configured() {
    let receiver_endpoint = match std::env::var("R3AKT_RETICULUMD_RPC_ENDPOINT") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!("skipping live field-command test: R3AKT_RETICULUMD_RPC_ENDPOINT is unset");
            return;
        }
    };
    let sender_endpoint = match std::env::var("R3AKT_RETICULUMD_FIELD_COMMAND_RPC_ENDPOINT") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!(
                "skipping live field-command test: R3AKT_RETICULUMD_FIELD_COMMAND_RPC_ENDPOINT is unset"
            );
            return;
        }
    };
    let source = match std::env::var("R3AKT_RETICULUMD_FIELD_COMMAND_SOURCE") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!(
                "skipping live field-command test: R3AKT_RETICULUMD_FIELD_COMMAND_SOURCE is unset"
            );
            return;
        }
    };
    let destination = match std::env::var("R3AKT_RETICULUMD_FIELD_COMMAND_DESTINATION") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!(
                "skipping live field-command test: R3AKT_RETICULUMD_FIELD_COMMAND_DESTINATION is unset"
            );
            return;
        }
    };
    let poll_attempts = std::env::var("R3AKT_RETICULUMD_RECEIPT_POLL_ATTEMPTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(120);
    let poll_delay_ms = std::env::var("R3AKT_RETICULUMD_RECEIPT_POLL_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(500);
    let db_path = std::env::temp_dir().join(format!(
        "r3akt-rch-live-reticulumd-field-command-{}.db",
        Uuid::new_v4()
    ));
    let state = crate::AppState::from_sqlite_path(&db_path)
        .expect("state")
        .with_reticulumd_rpc(receiver_endpoint.clone(), destination.clone());
    let command_id = format!("r3akt-live-field-command-{}", Uuid::new_v4());
    let mut sender_rpc = ReticulumdRpcClient::new(sender_endpoint.clone());
    let response = sender_rpc
        .call(
            "send_message_v2",
            Some(json!({
                "id": command_id,
                "source": source,
                "destination": destination,
                "title": "RCH",
                "content": "",
                "fields": {"9": [{"Command": "join"}]},
                "method": "direct"
            })),
        )
        .expect("send live field command");
    assert!(
        response.error.is_none(),
        "field command send failed: {response:?}"
    );

    let mut joined = false;
    for _ in 0..poll_attempts {
        crate::process_reticulumd_list_message_worker_tick(
            &state,
            receiver_endpoint.as_str(),
            destination.as_str(),
        )
        .expect("poll receiver messages");
        joined = state.clients.read().expect("clients").contains_key(
            crate::normalize_identity_key(source.as_str())
                .as_deref()
                .unwrap_or(source.as_str()),
        );
        if joined {
            break;
        }
        tokio::time::sleep(Duration::from_millis(poll_delay_ms)).await;
    }
    assert!(joined, "field-command join did not reach RCH");

    let mut reply = None;
    for _ in 0..poll_attempts {
        let response = sender_rpc
            .call("list_messages", Some(json!({"limit": 64})))
            .expect("poll sender messages");
        if response.error.is_none() {
            reply = response
                .result
                .as_ref()
                .and_then(|result| result.get("messages"))
                .and_then(Value::as_array)
                .and_then(|messages| {
                    messages.iter().find(|message| {
                        message.get("content").and_then(Value::as_str) == Some("joined")
                            && message.get("source").and_then(Value::as_str)
                                == Some(destination.as_str())
                            && message.get("destination").and_then(Value::as_str)
                                == Some(source.as_str())
                    })
                })
                .cloned();
        }
        if reply.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(poll_delay_ms)).await;
    }
    assert!(reply.is_some(), "field-command reply was not delivered");
    assert!(
        state
            .messages
            .read()
            .expect("messages")
            .iter()
            .any(|message| message.content == "joined")
    );

    let _ = std::fs::remove_file(db_path);
}
