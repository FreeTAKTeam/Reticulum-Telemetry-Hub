use std::collections::BTreeMap;

use r3akt_profile_rch::{
    CommandResultEnvelope, CommandResultStatus, FIELD_GROUP, FIELD_RESULTS, decode_results,
};

#[test]
fn team_scoped_result_field_ignores_group_metadata() {
    let result = CommandResultEnvelope {
        command_id: "cmd-team-result".to_string(),
        status: CommandResultStatus::Completed,
        detail: None,
        reason_code: None,
        reason: None,
        required_capabilities: Vec::new(),
        accepted_at: None,
        by_identity: None,
        correlation_id: Some("corr-team-result".to_string()),
        result: serde_json::json!({ "ok": true }),
    };
    let fields = BTreeMap::from([
        (FIELD_RESULTS, serde_json::json!([result.clone()])),
        (
            FIELD_GROUP,
            serde_json::json!("d6b6e188b910d6bdd24d04b7a7ec5444"),
        ),
    ]);
    let bytes = rmp_serde::to_vec_named(&fields).expect("encode");

    assert_eq!(decode_results(&bytes).expect("decode"), vec![result]);
}
