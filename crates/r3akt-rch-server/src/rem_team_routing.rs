use r3akt_profile_rch::FIELD_GROUP;
use serde_json::{Value, json};

use super::{
    ApiError, AppState, OutboundMessageRecord, mission_team_member_destinations,
    mission_uid_from_response_fields, record_outbound_message_with_metadata,
};

pub(super) fn send_mission_sync_response_to_source(
    state: &AppState,
    source: &str,
    topic: &str,
    command: &r3akt_protocol::Command,
    response: &r3akt_rch_core::MissionSyncResponse,
    team_uid: Option<&str>,
) -> Result<OutboundMessageRecord, ApiError> {
    let lxmf_fields = mission_response_fields(response, team_uid)?;
    record_outbound_message_with_metadata(
        state,
        response.content.as_str(),
        None,
        Some(source.to_string()),
        Vec::new(),
        true,
        json!({
            "reticulumd_inbound_command_reply": true,
            "source": "r3akt-rch-server",
            "direction": "outbound",
            "inbound_source": source,
            "inbound_topic": topic,
            "command": command.name,
            "correlation_id": command.correlation_id,
            "lxmf_fields": lxmf_fields,
        }),
    )
}

fn mission_response_fields(
    response: &r3akt_rch_core::MissionSyncResponse,
    team_uid: Option<&str>,
) -> Result<Value, ApiError> {
    let mut fields = serde_json::to_value(&response.fields).map_err(|error| {
        ApiError::Internal(format!(
            "failed to serialize mission response LXMF fields: {error}"
        ))
    })?;
    if let (Some(team_uid), Some(fields)) = (team_uid, fields.as_object_mut()) {
        fields.insert(FIELD_GROUP.to_string(), json!(team_uid));
    }
    Ok(fields)
}

pub(super) fn fanout_mission_sync_response_to_team(
    state: &AppState,
    response: &r3akt_rch_core::MissionSyncResponse,
    team_uid: Option<&str>,
    team_destinations: Option<&[String]>,
) -> Result<(), ApiError> {
    let Some(event) = response.event_field() else {
        return Ok(());
    };
    let mission_uid = mission_uid_from_response_fields(response);
    let destinations = if let Some(destinations) = team_destinations {
        destinations.to_vec()
    } else {
        let Some(mission_uid) = mission_uid.as_deref() else {
            return Ok(());
        };
        mission_team_member_destinations(state, mission_uid)?
    };
    if destinations.is_empty() {
        return Ok(());
    }
    let lxmf_fields = mission_response_fields(response, team_uid)?;
    let event_type = event
        .get("event_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    for destination in destinations {
        record_outbound_message_with_metadata(
            state,
            format!("r3akt mission event {event_type}").trim(),
            None,
            Some(destination),
            Vec::new(),
            true,
            json!({
                "reticulumd_inbound_mission_team_fanout": true,
                "source": "r3akt-rch-server",
                "direction": "outbound",
                "mission_uid": mission_uid,
                "team_uid": team_uid,
                "lxmf_fields": lxmf_fields.clone(),
            }),
        )?;
    }
    Ok(())
}
