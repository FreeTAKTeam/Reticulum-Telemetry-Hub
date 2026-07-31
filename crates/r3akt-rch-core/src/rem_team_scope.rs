use serde_json::Value;

use super::{MissionCommandEnvelope, MissionSyncResponse, RchCore, canonical_team_for_uid};

impl RchCore {
    pub(super) fn rem_team_scope_rejection(
        &self,
        command: &MissionCommandEnvelope,
    ) -> Option<MissionSyncResponse> {
        let (reason_code, reason) = self.validate_rem_team_scope(command).err()?;
        Some(MissionSyncResponse::results(Self::rejected_result(
            command,
            reason_code,
            reason,
        )))
    }

    pub(super) fn rem_registry_team_scope_rejection(
        &self,
        command: &MissionCommandEnvelope,
    ) -> Option<MissionSyncResponse> {
        let (reason_code, reason) = self.validate_rem_team_scope(command).err()?;
        Some(MissionSyncResponse::rem_results(Self::rejected_result(
            command,
            reason_code,
            reason,
        )))
    }

    fn validate_rem_team_scope(
        &self,
        command: &MissionCommandEnvelope,
    ) -> Result<(), (&'static str, String)> {
        let Some(team_uid) = command
            .args
            .get("_rem_team_uid")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(());
        };
        if canonical_team_for_uid(team_uid).is_none() {
            return Err((
                "invalid_team",
                "FIELD_GROUP must contain a canonical TEAM UID".to_string(),
            ));
        }
        if !self
            .shared_team_uids_for_rem_source(&command.source.rns_identity)
            .contains(team_uid)
        {
            return Err((
                "unauthorized_team",
                "The REM caller is not a member of the requested TEAM".to_string(),
            ));
        }
        Ok(())
    }
}
