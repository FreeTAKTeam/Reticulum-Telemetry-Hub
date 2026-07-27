use r3akt_profile_rch::{MissionCommandEnvelope, RchSource};
use serde_json::{Value, json};

use super::*;

fn command_from(source_identity: &str, command_type: &str, args: Value) -> MissionCommandEnvelope {
    let suffix = args.to_string();
    MissionCommandEnvelope {
        command_id: format!("cmd-{command_type}-{suffix}"),
        source: RchSource {
            rns_identity: source_identity.to_string(),
            display_name: Some("Field Agent".to_string()),
        },
        timestamp: "2026-05-03T12:00:00Z".to_string(),
        command_type: command_type.to_string(),
        args,
        correlation_id: Some("corr-1".to_string()),
        topics: Vec::new(),
    }
}

#[test]
fn rem_team_peer_registry_returns_only_recent_shared_team_rem_destinations() {
    const YELLOW_TEAM_UID: &str = "d6b6e188b910d6bdd24d04b7a7ec5444";
    const BLUE_TEAM_UID: &str = "43341e5c822d99857fa6e8641f2ca9c0";
    const CALLER_IDENTITY: &str = "11111111111111111111111111111111";
    const CALLER_CLIENT_IDENTITY: &str = "99999999999999999999999999999999";
    const CALLER_DESTINATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TEAMMATE_IDENTITY: &str = "22222222222222222222222222222222";
    const TEAMMATE_DESTINATION: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const LINKED_MEMBER_IDENTITY: &str = "33333333333333333333333333333333";
    const LINKED_CLIENT_IDENTITY: &str = "44444444444444444444444444444444";
    const LINKED_DESTINATION: &str = "cccccccccccccccccccccccccccccccc";
    const OUTSIDER_IDENTITY: &str = "55555555555555555555555555555555";
    const BLOCKED_IDENTITY: &str = "66666666666666666666666666666666";
    const STALE_IDENTITY: &str = "77777777777777777777777777777777";
    const GENERIC_IDENTITY: &str = "88888888888888888888888888888888";

    let mut core = RchCore::new();
    for (uid, name) in [(YELLOW_TEAM_UID, "Yellow"), (BLUE_TEAM_UID, "Blue")] {
        core.handle_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": uid, "team_name": name }),
        ));
    }
    for (uid, team_uid, identity) in [
        ("member-caller", YELLOW_TEAM_UID, CALLER_IDENTITY),
        ("member-teammate", YELLOW_TEAM_UID, TEAMMATE_IDENTITY),
        ("member-linked", YELLOW_TEAM_UID, LINKED_MEMBER_IDENTITY),
        ("member-blocked", YELLOW_TEAM_UID, BLOCKED_IDENTITY),
        ("member-stale", YELLOW_TEAM_UID, STALE_IDENTITY),
        ("member-generic", YELLOW_TEAM_UID, GENERIC_IDENTITY),
        ("member-outsider", BLUE_TEAM_UID, OUTSIDER_IDENTITY),
    ] {
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({
                "uid": uid,
                "team_uid": team_uid,
                "rns_identity": identity,
                "display_name": uid,
            }),
        ));
    }
    core.handle_command(&command(
        "mission.registry.team_member.client.link",
        json!({
            "team_member_uid": "member-linked",
            "client_identity": LINKED_CLIENT_IDENTITY,
        }),
    ));
    core.handle_command(&command(
        "mission.registry.team_member.client.link",
        json!({
            "team_member_uid": "member-caller",
            "client_identity": CALLER_CLIENT_IDENTITY,
        }),
    ));

    let rem_capabilities = vec![
        "r3akt".to_string(),
        "EmergencyMessages".to_string(),
        "Telemetry".to_string(),
    ];
    for (identity, destination, name) in [
        (CALLER_CLIENT_IDENTITY, CALLER_DESTINATION, "Caller"),
        (TEAMMATE_IDENTITY, TEAMMATE_DESTINATION, "Teammate"),
        (LINKED_CLIENT_IDENTITY, LINKED_DESTINATION, "Linked client"),
    ] {
        core.record_identity_announce(
            identity,
            None,
            Some(format!("{name} identity")),
            Some("identity".to_string()),
            rem_capabilities.clone(),
        )
        .expect("identity announce");
        core.record_identity_announce(
            destination,
            Some(identity.to_string()),
            Some(format!("{name} destination")),
            Some("destination".to_string()),
            rem_capabilities.clone(),
        )
        .expect("destination announce");
    }
    for identity in [OUTSIDER_IDENTITY, BLOCKED_IDENTITY, STALE_IDENTITY] {
        core.record_identity_announce(
            identity,
            None,
            Some(identity.to_string()),
            Some("identity".to_string()),
            rem_capabilities.clone(),
        )
        .expect("REM announce");
    }
    core.record_identity_announce(
        GENERIC_IDENTITY,
        None,
        Some("Generic client".to_string()),
        Some("identity".to_string()),
        vec!["r3akt".to_string()],
    )
    .expect("generic announce");
    core.set_identity_state(BLOCKED_IDENTITY, true, false)
        .expect("blocked state");
    core.identity_announces
        .get_mut(STALE_IDENTITY)
        .expect("stale announce")
        .last_seen_ts_ms = utc_now_ms() - RECENT_ANNOUNCE_WINDOW_MS - 1;
    core.set_identity_rem_mode(LINKED_DESTINATION, "semi_autonomous")
        .expect("teammate mode");
    core.set_identity_rem_mode(OUTSIDER_IDENTITY, "connected")
        .expect("outsider connected mode");

    let responses = core.handle_mission_sync_command(&command_from(
        CALLER_DESTINATION,
        "rem.registry.team_peers.list",
        json!({}),
    ));

    assert_eq!(responses.len(), 2);
    assert_eq!(
        responses[0].results_field().expect("accepted")["status"],
        "accepted"
    );
    assert_eq!(
        responses[1].event_field().expect("event")["event_type"],
        "rem.registry.team_peers.listed"
    );
    let result = &responses[1].results_field().expect("result")["result"];
    assert_eq!(result["schema_version"], 2);
    assert_eq!(result["scope"], "shared_teams");
    assert_eq!(result["effective_connected_mode"], false);
    assert_eq!(
        result["teams"],
        json!([{
            "uid": YELLOW_TEAM_UID,
            "color": "YELLOW",
            "team_name": "YELLOW",
        }])
    );
    assert_eq!(
        result["caller_memberships"],
        json!([{
            "team_uid": YELLOW_TEAM_UID,
            "team_member_uid": "member-caller",
        }])
    );
    let members = result["members"].as_array().expect("members");
    assert_eq!(members.len(), 3);
    assert!(members.iter().any(|member| {
        member["identity"] == STALE_IDENTITY
            && member["team_uid"] == YELLOW_TEAM_UID
            && member["team_member_uid"] == "member-stale"
            && member["status"] == "offline"
    }));
    assert!(
        members
            .iter()
            .all(|member| member["identity"] != BLOCKED_IDENTITY)
    );
    assert!(
        members
            .iter()
            .all(|member| member["identity"] != GENERIC_IDENTITY)
    );
    assert!(
        members
            .iter()
            .all(|member| member["identity"] != CALLER_CLIENT_IDENTITY)
    );
    let items = result["items"].as_array().expect("items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["identity"], TEAMMATE_IDENTITY);
    assert_eq!(items[0]["destination_hash"], TEAMMATE_DESTINATION);
    assert_eq!(items[0]["display_name"], "Teammate destination");
    assert_eq!(items[1]["identity"], LINKED_CLIENT_IDENTITY);
    assert_eq!(items[1]["destination_hash"], LINKED_DESTINATION);
    assert_eq!(items[1]["registered_mode"], "semi_autonomous");
    assert!(items.iter().all(|item| item["client_type"] == "rem"));
    assert!(items.iter().all(|item| item["status"] == "active"));

    let routed = core.rem_team_routing_destinations(CALLER_DESTINATION, YELLOW_TEAM_UID);
    assert!(routed.contains(&TEAMMATE_DESTINATION.to_string()));
    assert!(routed.contains(&LINKED_DESTINATION.to_string()));
    assert!(!routed.contains(&CALLER_DESTINATION.to_string()));
    assert!(
        core.rem_team_routing_destinations(CALLER_DESTINATION, BLUE_TEAM_UID)
            .is_empty()
    );

    let denied_registry = core.handle_mission_sync_command(&command_from(
        CALLER_DESTINATION,
        "rem.registry.team_peers.list",
        json!({ "_rem_team_uid": BLUE_TEAM_UID }),
    ));
    let denied_registry_result = denied_registry[0]
        .results_field()
        .expect("REM team rejection");
    assert_eq!(denied_registry_result["status"], "rejected");
    assert_eq!(denied_registry_result["reason_code"], "unauthorized_team");

    let denied = core.handle_mission_sync_command(&command_from(
        CALLER_DESTINATION,
        "mission.registry.eam.list",
        json!({ "_rem_team_uid": BLUE_TEAM_UID }),
    ));
    let denied_result = denied[0].results_field().expect("team rejection");
    assert_eq!(denied_result["status"], "rejected");
    assert_eq!(denied_result["reason_code"], "unauthorized_team");
}

#[test]
fn rem_team_peer_registry_preserves_multi_team_membership_rows() {
    const YELLOW_TEAM_UID: &str = "d6b6e188b910d6bdd24d04b7a7ec5444";
    const BLUE_TEAM_UID: &str = "43341e5c822d99857fa6e8641f2ca9c0";
    const CALLER_IDENTITY: &str = "11111111111111111111111111111111";
    const CALLER_DESTINATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PEER_IDENTITY: &str = "22222222222222222222222222222222";
    const PEER_DESTINATION: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    let mut core = RchCore::new();
    for (uid, color) in [(YELLOW_TEAM_UID, "Yellow"), (BLUE_TEAM_UID, "Blue")] {
        core.handle_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": uid, "team_name": color }),
        ));
    }
    for (uid, team_uid, identity) in [
        ("caller-yellow", YELLOW_TEAM_UID, CALLER_IDENTITY),
        ("caller-blue", BLUE_TEAM_UID, CALLER_IDENTITY),
        ("peer-yellow", YELLOW_TEAM_UID, PEER_IDENTITY),
        ("peer-blue", BLUE_TEAM_UID, PEER_IDENTITY),
    ] {
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({
                "uid": uid,
                "team_uid": team_uid,
                "rns_identity": identity,
                "display_name": uid,
            }),
        ));
    }
    let capabilities = vec!["r3akt".to_string(), "EmergencyMessages".to_string()];
    for (identity, destination) in [
        (CALLER_IDENTITY, CALLER_DESTINATION),
        (PEER_IDENTITY, PEER_DESTINATION),
    ] {
        core.record_identity_announce(
            destination,
            Some(identity.to_string()),
            Some(identity.to_string()),
            Some("destination".to_string()),
            capabilities.clone(),
        )
        .expect("destination announce");
    }

    let responses = core.handle_mission_sync_command(&command_from(
        CALLER_DESTINATION,
        "rem.registry.team_peers.list",
        json!({}),
    ));
    let result = &responses[1].results_field().expect("result")["result"];
    let caller_memberships = result["caller_memberships"]
        .as_array()
        .expect("caller memberships");
    assert_eq!(caller_memberships.len(), 2);
    let members = result["members"].as_array().expect("members");
    assert_eq!(members.len(), 2);
    assert!(members.iter().any(|member| {
        member["team_uid"] == YELLOW_TEAM_UID && member["team_member_uid"] == "peer-yellow"
    }));
    assert!(members.iter().any(|member| {
        member["team_uid"] == BLUE_TEAM_UID && member["team_member_uid"] == "peer-blue"
    }));
}

#[test]
fn rem_team_peer_registry_rejects_rem_callers_without_team_membership() {
    const CALLER_IDENTITY: &str = "11111111111111111111111111111111";
    const CALLER_DESTINATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut core = RchCore::new();
    core.record_identity_announce(
        CALLER_DESTINATION,
        Some(CALLER_IDENTITY.to_string()),
        Some("Unrostered REM".to_string()),
        Some("destination".to_string()),
        vec!["r3akt".to_string(), "EmergencyMessages".to_string()],
    )
    .expect("caller announce");

    let responses = core.handle_mission_sync_command(&command_from(
        CALLER_DESTINATION,
        "rem.registry.team_peers.list",
        json!({}),
    ));

    assert_eq!(responses.len(), 1);
    let result = responses[0].results_field().expect("rejected");
    assert_eq!(result["status"], "rejected");
    assert_eq!(result["reason_code"], "unauthorized");
    assert_eq!(result["reason"], "TEAM membership is required");
}
