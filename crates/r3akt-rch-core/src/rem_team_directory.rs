use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::{
    IdentityAnnounceRecord, RECENT_ANNOUNCE_WINDOW_MS, RchCore, TeamMemberRecord,
    canonical_team_for_uid, millis_to_rfc3339, normalize_hash, utc_now_ms,
};

const REM_TEAM_DIRECTORY_SCHEMA_VERSION: u64 = 2;

impl RchCore {
    /// Returns validated REM destinations in one canonical TEAM for Connected-mode fanout.
    /// The caller must itself belong to the requested TEAM.
    #[must_use]
    pub fn rem_team_routing_destinations(&self, source: &str, team_uid: &str) -> Vec<String> {
        if canonical_team_for_uid(team_uid).is_none()
            || !self
                .shared_team_uids_for_rem_source(source)
                .contains(team_uid)
        {
            return Vec::new();
        }
        let team_uids = HashSet::from([team_uid.to_string()]);
        let requester_identity = self.canonical_identity_for_rem_source(source);
        let requester_destination = normalize_hash(Some(source));
        let cutoff_ms = utc_now_ms().saturating_sub(RECENT_ANNOUNCE_WINDOW_MS);
        let rem_modes = self
            .identity_rem_modes
            .iter()
            .map(|(identity, record)| (identity.clone(), record.mode.trim().to_ascii_lowercase()))
            .collect::<HashMap<_, _>>();
        self.durable_team_directory_members(
            &team_uids,
            requester_identity.as_deref(),
            requester_destination.as_deref(),
            &rem_modes,
            cutoff_ms,
        )
        .into_iter()
        .filter_map(|member| member["destination_hash"].as_str().map(ToString::to_string))
        .collect()
    }

    fn identity_announce_has_rem_capabilities(record: &IdentityAnnounceRecord) -> bool {
        let capabilities: HashSet<_> = record
            .announce_capabilities
            .iter()
            .map(String::as_str)
            .collect();
        capabilities.contains("r3akt") && capabilities.contains("emergencymessages")
    }

    pub(super) fn identity_has_rem_announce_capabilities(&self, identity: &str) -> bool {
        self.identity_announce_for_identity(identity)
            .is_some_and(Self::identity_announce_has_rem_capabilities)
    }

    fn identity_announce_for_identity(&self, identity: &str) -> Option<&IdentityAnnounceRecord> {
        let identity = normalize_hash(Some(identity))?;
        if let Some(record) = self.identity_announces.get(&identity) {
            return Some(record);
        }
        self.identity_announces
            .values()
            .filter(|record| {
                record
                    .announced_identity_hash
                    .as_deref()
                    .is_some_and(|announced| announced == identity)
            })
            .max_by_key(|record| {
                (
                    record.source_interface.as_deref() == Some("identity"),
                    record.display_name.is_some(),
                    record.last_seen_ts_ms,
                )
            })
    }

    fn canonical_identity_for_rem_source(&self, source: &str) -> Option<String> {
        let source = normalize_hash(Some(source))?;
        self.identity_announce_for_identity(&source)
            .and_then(|record| {
                record
                    .announced_identity_hash
                    .as_deref()
                    .and_then(|identity| normalize_hash(Some(identity)))
            })
            .or(Some(source))
    }

    pub(super) fn shared_team_uids_for_rem_source(&self, source: &str) -> HashSet<String> {
        let Some(identity) = self.canonical_identity_for_rem_source(source) else {
            return HashSet::new();
        };
        self.team_members
            .values()
            .filter(|member| {
                normalize_hash(Some(&member.rns_identity)).as_deref() == Some(identity.as_str())
                    || member.client_identities.iter().any(|client_identity| {
                        normalize_hash(Some(client_identity)).as_deref() == Some(identity.as_str())
                    })
                    || self
                        .team_member_client_links
                        .contains(&(member.uid.clone(), identity.clone()))
            })
            .filter_map(|member| member.team_uid.clone())
            .collect()
    }

    fn caller_memberships_for_rem_source(&self, source: &str) -> Vec<Value> {
        let Some(identity) = self.canonical_identity_for_rem_source(source) else {
            return Vec::new();
        };
        let mut memberships = self
            .team_members
            .values()
            .filter(|member| Self::member_has_identity(self, member, &identity))
            .filter_map(|member| {
                let team_uid = member.team_uid.as_deref()?;
                canonical_team_for_uid(team_uid)?;
                Some(json!({
                    "team_uid": team_uid,
                    "team_member_uid": member.uid,
                }))
            })
            .collect::<Vec<_>>();
        memberships.sort_by(|left, right| {
            left["team_uid"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["team_uid"].as_str().unwrap_or_default())
                .then_with(|| {
                    left["team_member_uid"]
                        .as_str()
                        .unwrap_or_default()
                        .cmp(right["team_member_uid"].as_str().unwrap_or_default())
                })
        });
        memberships
    }

    fn member_has_identity(&self, member: &TeamMemberRecord, identity: &str) -> bool {
        normalize_hash(Some(&member.rns_identity)).as_deref() == Some(identity)
            || member.client_identities.iter().any(|client_identity| {
                normalize_hash(Some(client_identity)).as_deref() == Some(identity)
            })
            || self
                .team_member_client_links
                .contains(&(member.uid.clone(), identity.to_string()))
    }

    fn member_identities(&self, member: &TeamMemberRecord) -> HashSet<String> {
        let mut identities = HashSet::new();
        if let Some(identity) = normalize_hash(Some(&member.rns_identity)) {
            identities.insert(identity);
        }
        identities.extend(
            member
                .client_identities
                .iter()
                .filter_map(|identity| normalize_hash(Some(identity))),
        );
        identities.extend(
            self.team_member_client_links
                .iter()
                .filter(|(member_uid, _)| member_uid == &member.uid)
                .filter_map(|(_, identity)| normalize_hash(Some(identity))),
        );
        identities
    }

    fn team_member_identities_for_teams(&self, team_uids: &HashSet<String>) -> HashSet<String> {
        let mut identities = HashSet::new();
        for member in self.team_members.values().filter(|member| {
            member
                .team_uid
                .as_ref()
                .is_some_and(|team_uid| team_uids.contains(team_uid))
        }) {
            if let Some(identity) = normalize_hash(Some(&member.rns_identity)) {
                identities.insert(identity);
            }
            identities.extend(
                member
                    .client_identities
                    .iter()
                    .filter_map(|identity| normalize_hash(Some(identity))),
            );
            identities.extend(
                self.team_member_client_links
                    .iter()
                    .filter(|(member_uid, _)| member_uid == &member.uid)
                    .filter_map(|(_, identity)| normalize_hash(Some(identity))),
            );
        }
        identities
    }

    fn best_rem_announce_for_identity(&self, identity: &str) -> Option<&IdentityAnnounceRecord> {
        self.identity_announces
            .values()
            .filter(|record| {
                record
                    .announced_identity_hash
                    .as_deref()
                    .and_then(|value| normalize_hash(Some(value)))
                    .or_else(|| normalize_hash(Some(&record.destination_hash)))
                    .as_deref()
                    == Some(identity)
                    && record.client_type.trim().eq_ignore_ascii_case("rem")
                    && Self::identity_announce_has_rem_capabilities(record)
            })
            .max_by_key(|record| {
                (
                    record.source_interface.as_deref() == Some("destination"),
                    record.last_seen_ts_ms,
                )
            })
    }

    fn rem_mode_for_announce(
        rem_modes: &HashMap<String, String>,
        identity: &str,
        record: &IdentityAnnounceRecord,
    ) -> String {
        rem_modes
            .get(identity)
            .or_else(|| {
                normalize_hash(Some(&record.destination_hash))
                    .as_ref()
                    .and_then(|destination| rem_modes.get(destination))
            })
            .cloned()
            .unwrap_or_else(|| "autonomous".to_string())
    }

    fn durable_team_directory_members(
        &self,
        team_uids: &HashSet<String>,
        requester_identity: Option<&str>,
        requester_destination: Option<&str>,
        rem_modes: &HashMap<String, String>,
        cutoff_ms: i64,
    ) -> Vec<Value> {
        let mut members = Vec::new();
        let mut seen = HashSet::new();
        for member in self.team_members.values() {
            let Some(team_uid) = member.team_uid.as_deref() else {
                continue;
            };
            if !team_uids.contains(team_uid) || canonical_team_for_uid(team_uid).is_none() {
                continue;
            }
            for identity in self.member_identities(member) {
                if requester_identity == Some(identity.as_str()) {
                    continue;
                }
                if self
                    .identity_states
                    .get(&identity)
                    .is_some_and(|state| state.is_banned || state.is_blackholed)
                {
                    continue;
                }
                let Some(record) = self.best_rem_announce_for_identity(&identity) else {
                    continue;
                };
                let destination_hash = normalize_hash(Some(&record.destination_hash))
                    .unwrap_or_else(|| identity.clone());
                if requester_destination == Some(destination_hash.as_str())
                    || self
                        .identity_states
                        .get(&destination_hash)
                        .is_some_and(|state| state.is_banned || state.is_blackholed)
                {
                    continue;
                }
                if !seen.insert((
                    team_uid.to_string(),
                    member.uid.clone(),
                    identity.clone(),
                    destination_hash.clone(),
                )) {
                    continue;
                }
                members.push(json!({
                    "team_uid": team_uid,
                    "team_member_uid": member.uid,
                    "identity": identity,
                    "destination_hash": destination_hash,
                    "display_name": record
                        .display_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .or_else(|| (!member.display_name.trim().is_empty()).then_some(member.display_name.trim())),
                    "announce_capabilities": record.announce_capabilities.clone(),
                    "client_type": "rem",
                    "registered_mode": Self::rem_mode_for_announce(rem_modes, &identity, record),
                    "last_seen": millis_to_rfc3339(record.last_seen_ts_ms),
                    "status": if record.last_seen_ts_ms >= cutoff_ms { "active" } else { "offline" },
                }));
            }
        }
        members.sort_by(|left, right| {
            left["team_uid"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["team_uid"].as_str().unwrap_or_default())
                .then_with(|| {
                    left["team_member_uid"]
                        .as_str()
                        .unwrap_or_default()
                        .cmp(right["team_member_uid"].as_str().unwrap_or_default())
                })
                .then_with(|| {
                    left["identity"]
                        .as_str()
                        .unwrap_or_default()
                        .cmp(right["identity"].as_str().unwrap_or_default())
                })
        });
        members
    }

    pub(super) fn rem_team_peer_registry_payload(&self, source: &str) -> Value {
        let team_uids = self.shared_team_uids_for_rem_source(source);
        let team_identities = self.team_member_identities_for_teams(&team_uids);
        let requester_identity = self.canonical_identity_for_rem_source(source);
        let requester_destination = normalize_hash(Some(source));
        let cutoff_ms = utc_now_ms().saturating_sub(RECENT_ANNOUNCE_WINDOW_MS);
        let rem_modes: HashMap<String, String> = self
            .identity_rem_modes
            .iter()
            .map(|(identity, record)| {
                let mode = record.mode.trim().to_ascii_lowercase();
                (
                    identity.clone(),
                    if mode.is_empty() {
                        "autonomous".to_string()
                    } else {
                        mode
                    },
                )
            })
            .collect();
        let canonical_team_uids = team_uids
            .iter()
            .filter(|team_uid| canonical_team_for_uid(team_uid).is_some())
            .cloned()
            .collect::<HashSet<_>>();
        let mut teams = canonical_team_uids
            .iter()
            .filter_map(|team_uid| {
                let (_, color) = canonical_team_for_uid(team_uid)?;
                let team_name = self
                    .teams
                    .get(team_uid)
                    .map(|team| team.team_name.trim())
                    .filter(|value| !value.is_empty())
                    .unwrap_or(color);
                Some(json!({
                    "uid": team_uid,
                    "color": color,
                    "team_name": team_name,
                }))
            })
            .collect::<Vec<_>>();
        teams.sort_by(|left, right| {
            left["color"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["color"].as_str().unwrap_or_default())
        });
        let caller_memberships = self.caller_memberships_for_rem_source(source);
        let members = self.durable_team_directory_members(
            &canonical_team_uids,
            requester_identity.as_deref(),
            requester_destination.as_deref(),
            &rem_modes,
            cutoff_ms,
        );
        let mut candidates: HashMap<String, (&IdentityAnnounceRecord, String)> = HashMap::new();
        for record in self.identity_announces.values() {
            let identity = record
                .announced_identity_hash
                .as_deref()
                .and_then(|value| normalize_hash(Some(value)))
                .or_else(|| normalize_hash(Some(&record.destination_hash)));
            let Some(identity) = identity else {
                continue;
            };
            if !team_identities.contains(&identity)
                || requester_identity.as_deref() == Some(identity.as_str())
                || requester_destination.as_deref()
                    == normalize_hash(Some(&record.destination_hash)).as_deref()
                || record.last_seen_ts_ms < cutoff_ms
                || !record.client_type.trim().eq_ignore_ascii_case("rem")
                || !Self::identity_announce_has_rem_capabilities(record)
            {
                continue;
            }
            if self
                .identity_states
                .get(&identity)
                .is_some_and(|state| state.is_banned || state.is_blackholed)
            {
                continue;
            }
            let source = record
                .source_interface
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map_or_else(|| "identity".to_string(), str::to_ascii_lowercase);
            let replace = candidates
                .get(&identity)
                .is_none_or(|(_, existing_source)| {
                    source == "destination" && existing_source != "destination"
                });
            if replace {
                candidates.insert(identity, (record, source));
            }
        }

        let effective_connected_mode = candidates.iter().any(|(identity, (record, _))| {
            rem_modes
                .get(identity)
                .or_else(|| {
                    normalize_hash(Some(&record.destination_hash))
                        .as_ref()
                        .and_then(|destination| rem_modes.get(destination))
                })
                .is_some_and(|mode| mode == "connected")
        });
        let mut items = candidates
            .into_iter()
            .map(|(identity, (record, source))| {
                let destination_hash = if source == "destination" {
                    normalize_hash(Some(&record.destination_hash))
                        .unwrap_or_else(|| identity.clone())
                } else {
                    identity.clone()
                };
                json!({
                    "identity": identity.clone(),
                    "destination_hash": destination_hash,
                    "display_name": record
                        .display_name
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty()),
                    "announce_capabilities": record.announce_capabilities.clone(),
                    "client_type": record.client_type.trim().to_ascii_lowercase(),
                    "registered_mode": rem_modes
                        .get(&identity)
                        .or_else(|| {
                            normalize_hash(Some(&record.destination_hash))
                                .as_ref()
                                .and_then(|destination| rem_modes.get(destination))
                        })
                        .cloned()
                        .unwrap_or_else(|| "autonomous".to_string()),
                    "last_seen": millis_to_rfc3339(record.last_seen_ts_ms),
                    "status": "active",
                })
            })
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left["identity"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["identity"].as_str().unwrap_or_default())
        });

        json!({
            "schema_version": REM_TEAM_DIRECTORY_SCHEMA_VERSION,
            "scope": "shared_teams",
            "effective_connected_mode": effective_connected_mode,
            "teams": teams,
            "caller_memberships": caller_memberships,
            "members": members,
            "items": items,
        })
    }
}
