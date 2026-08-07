#![allow(
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::too_many_lines
)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::let_underscore_must_use,
        clippy::panic,
        clippy::unwrap_used
    )
)]

mod text;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use chrono::{DateTime, SecondsFormat, Utc};
use r3akt_profile_rch::{
    CommandResultEnvelope, CommandResultStatus, EventEnvelope, MissionCommandEnvelope,
};
use r3akt_protocol::{Destination, NodeId, Payload, ProtocolEnvelope, Topic, TopicMessage};
pub use r3akt_shared_mesh_delivery::{
    DEFAULT_PRIORITY, DEFAULT_TTL_SECONDS, DELIVERY_SCHEMA_VERSION, DeliveryEnvelope, DeliveryMode,
    OutboundDeliveryDecision, OutboundDeliveryPolicy, RECENT_ANNOUNCE_WINDOW_MS,
    RECENT_RUNTIME_PRESENCE_WINDOW_MS,
};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags, Transaction, params, params_from_iter};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use text::decode_utf8_ignoring_errors;

pub mod python_migration;
mod rem_team_directory;
mod rem_team_scope;

pub const DELIVERY_ENVELOPE_FIELD: &str = "RTHDelivery";
pub const MAX_CLOCK_SKEW_SECONDS: i64 = 300;
pub const DEFAULT_ROSTER_ROLE: &str = "user";
const DEFAULT_LOG_MISSION_UID: &str = "mission-default";

pub const ROLE_FIELD_OPERATOR: &str = "FIELD_OPERATOR";
pub const ROLE_TEAM_LEAD: &str = "TEAM_LEAD";
pub const ROLE_INCIDENT_COMMANDER: &str = "INCIDENT_COMMANDER";
pub const ROLE_LOGISTICS_RESOURCE_MANAGER: &str = "LOGISTICS_RESOURCE_MANAGER";
pub const ROLE_COMMUNICATIONS_OPERATOR: &str = "COMMUNICATIONS_OPERATOR";
pub const ROLE_SYSTEM_ADMIN: &str = "SYSTEM_ADMIN";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RchRoleBundleDefinition {
    pub role: &'static str,
    pub label: &'static str,
    pub persona: &'static str,
    pub description: &'static str,
    pub scope_types: &'static [&'static str],
    pub needs: &'static [&'static str],
    pub operations: &'static [&'static str],
}

const MISSION_READONLY_OPERATION_LIST: &[&str] = &[
    "checklist.join",
    "checklist.read",
    "mission.audit.read",
    "mission.content.read",
    "mission.join",
    "mission.leave",
    "mission.pause",
    "mission.resume",
    "mission.nick",
    "mission.users",
    "mission.registry.asset.read",
    "mission.registry.assignment.read",
    "mission.registry.log.read",
    "mission.registry.mission.read",
    "mission.registry.skill.read",
    "mission.registry.status.read",
    "mission.registry.team.read",
    "mission.zone.read",
    "topic.read",
];

const MISSION_WRITE_OPERATION_LIST: &[&str] = &[
    "checklist.feed.publish",
    "checklist.join",
    "checklist.read",
    "checklist.upload",
    "checklist.write",
    "mission.audit.read",
    "mission.content.read",
    "mission.content.write",
    "mission.join",
    "mission.leave",
    "mission.pause",
    "mission.resume",
    "mission.nick",
    "mission.users",
    "mission.message.send",
    "mission.registry.assignment.read",
    "mission.registry.assignment.write",
    "mission.registry.asset.read",
    "mission.registry.log.read",
    "mission.registry.log.write",
    "mission.registry.mission.read",
    "mission.registry.skill.read",
    "mission.registry.status.read",
    "mission.registry.status.write",
    "mission.registry.team.read",
    "mission.zone.read",
    "mission.zone.write",
    "topic.read",
    "topic.subscribe",
];

const MISSION_OWNER_OPERATION_LIST: &[&str] = &[
    "checklist.feed.publish",
    "checklist.join",
    "checklist.read",
    "checklist.upload",
    "checklist.write",
    "mission.audit.read",
    "mission.content.read",
    "mission.content.write",
    "mission.join",
    "mission.leave",
    "mission.pause",
    "mission.resume",
    "mission.nick",
    "mission.users",
    "mission.message.send",
    "mission.registry.asset.read",
    "mission.registry.asset.write",
    "mission.registry.assignment.read",
    "mission.registry.assignment.write",
    "mission.registry.log.read",
    "mission.registry.log.write",
    "mission.registry.mission.read",
    "mission.registry.mission.write",
    "mission.registry.skill.read",
    "mission.registry.skill.write",
    "mission.registry.status.read",
    "mission.registry.status.write",
    "mission.registry.team.read",
    "mission.registry.team.write",
    "mission.zone.delete",
    "mission.zone.read",
    "mission.zone.write",
    "topic.create",
    "topic.delete",
    "topic.read",
    "topic.subscribe",
    "topic.write",
];

const FIELD_OPERATOR_OPERATION_LIST: &[&str] = &[
    "checklist.join",
    "checklist.read",
    "checklist.write",
    "emergency.alert.send",
    "mission.content.read",
    "mission.join",
    "mission.leave",
    "mission.message.send",
    "mission.registry.assignment.read",
    "mission.registry.assignment.write",
    "mission.registry.status.read",
    "mission.registry.status.write",
    "mission.zone.read",
    "topic.read",
    "topic.subscribe",
];

const TEAM_LEAD_OPERATION_LIST: &[&str] = &[
    "checklist.feed.publish",
    "checklist.join",
    "checklist.read",
    "checklist.upload",
    "checklist.write",
    "emergency.alert.send",
    "mission.audit.read",
    "mission.content.read",
    "mission.content.write",
    "mission.join",
    "mission.leave",
    "mission.message.send",
    "mission.registry.assignment.read",
    "mission.registry.assignment.write",
    "mission.registry.status.read",
    "mission.registry.status.write",
    "mission.registry.team.read",
    "mission.zone.read",
    "mission.zone.write",
    "topic.read",
    "topic.subscribe",
];

const INCIDENT_COMMANDER_OPERATION_LIST: &[&str] = &[
    "checklist.feed.publish",
    "checklist.join",
    "checklist.read",
    "checklist.template.read",
    "checklist.upload",
    "checklist.write",
    "emergency.alert.send",
    "mission.audit.read",
    "mission.content.read",
    "mission.content.write",
    "mission.join",
    "mission.leave",
    "mission.message.send",
    "mission.registry.asset.read",
    "mission.registry.asset.write",
    "mission.registry.assignment.read",
    "mission.registry.assignment.write",
    "mission.registry.log.read",
    "mission.registry.log.write",
    "mission.registry.mission.read",
    "mission.registry.mission.write",
    "mission.registry.skill.read",
    "mission.registry.skill.write",
    "mission.registry.status.read",
    "mission.registry.status.write",
    "mission.registry.team.read",
    "mission.registry.team.write",
    "mission.zone.delete",
    "mission.zone.read",
    "mission.zone.write",
    "topic.create",
    "topic.delete",
    "topic.read",
    "topic.subscribe",
    "topic.write",
];

const LOGISTICS_RESOURCE_MANAGER_OPERATION_LIST: &[&str] = &[
    "checklist.join",
    "checklist.read",
    "checklist.upload",
    "checklist.write",
    "mission.audit.read",
    "mission.content.read",
    "mission.join",
    "mission.leave",
    "mission.message.send",
    "mission.registry.asset.read",
    "mission.registry.asset.write",
    "mission.registry.assignment.read",
    "mission.registry.assignment.write",
    "mission.registry.log.read",
    "mission.registry.log.write",
    "mission.registry.skill.read",
    "mission.registry.status.read",
    "mission.registry.status.write",
    "mission.registry.team.read",
    "mission.zone.read",
    "topic.read",
    "topic.subscribe",
];

const COMMUNICATIONS_OPERATOR_OPERATION_LIST: &[&str] = &[
    "diagnostics.network.read",
    "mission.audit.read",
    "mission.content.read",
    "mission.join",
    "mission.leave",
    "mission.message.send",
    "mission.registry.log.read",
    "mission.registry.log.write",
    "mission.registry.status.read",
    "mission.registry.status.write",
    "mission.registry.team.read",
    "runtime.delivery.read",
    "runtime.node.read",
    "runtime.routing.read",
    "topic.create",
    "topic.delete",
    "topic.read",
    "topic.subscribe",
    "topic.write",
];

const SYSTEM_ADMIN_OPERATION_LIST: &[&str] = &[
    "admin.backup.write",
    "admin.config.write",
    "admin.enrollment.write",
    "admin.identity.revoke",
    "diagnostics.network.read",
    "mission.audit.read",
    "mission.registry.log.read",
    "mission.registry.mission.read",
    "mission.registry.status.read",
    "mission.registry.team.read",
    "mission.registry.team.write",
    "r3akt",
    "runtime.delivery.read",
    "runtime.node.read",
    "runtime.routing.read",
];

const RCH_ROLE_BUNDLES: &[RchRoleBundleDefinition] = &[
    RchRoleBundleDefinition {
        role: ROLE_FIELD_OPERATOR,
        label: "Field operator",
        persona: "Field operator",
        description: "Simple status buttons, task list, emergency alert, map, and team messages.",
        scope_types: &["mission"],
        needs: &[
            "Simple status buttons",
            "Task list",
            "Emergency alert",
            "Map",
            "Team messages",
        ],
        operations: FIELD_OPERATOR_OPERATION_LIST,
    },
    RchRoleBundleDefinition {
        role: ROLE_TEAM_LEAD,
        label: "Team lead",
        persona: "Team lead",
        description: "Assign tasks, monitor team status, and coordinate movement.",
        scope_types: &["mission"],
        needs: &["Assign tasks", "Monitor team status", "Coordinate movement"],
        operations: TEAM_LEAD_OPERATION_LIST,
    },
    RchRoleBundleDefinition {
        role: ROLE_INCIDENT_COMMANDER,
        label: "Incident commander",
        persona: "Incident commander",
        description: "Incident dashboard, objectives, resources, priorities, and audit log.",
        scope_types: &["mission"],
        needs: &[
            "Incident dashboard",
            "Objectives",
            "Resources",
            "Priorities",
            "Audit log",
        ],
        operations: INCIDENT_COMMANDER_OPERATION_LIST,
    },
    RchRoleBundleDefinition {
        role: ROLE_LOGISTICS_RESOURCE_MANAGER,
        label: "Logistics/resource manager",
        persona: "Logistics/resource manager",
        description: "Resource requests, staging, availability, and fulfillment.",
        scope_types: &["mission"],
        needs: &[
            "Resource requests",
            "Staging",
            "Availability",
            "Fulfillment",
        ],
        operations: LOGISTICS_RESOURCE_MANAGER_OPERATION_LIST,
    },
    RchRoleBundleDefinition {
        role: ROLE_COMMUNICATIONS_OPERATOR,
        label: "Communications operator",
        persona: "Communications operator",
        description: "Node health, routing status, message delivery, and degraded-network diagnostics.",
        scope_types: &["global", "mission"],
        needs: &[
            "Node health",
            "Routing status",
            "Message delivery",
            "Degraded-network diagnostics",
        ],
        operations: COMMUNICATIONS_OPERATOR_OPERATION_LIST,
    },
    RchRoleBundleDefinition {
        role: ROLE_SYSTEM_ADMIN,
        label: "System admin",
        persona: "System admin",
        description: "Enrollment, roles, revocation, configuration, and backups.",
        scope_types: &["global"],
        needs: &[
            "Enrollment",
            "Roles",
            "Revocation",
            "Configuration",
            "Backups",
        ],
        operations: SYSTEM_ADMIN_OPERATION_LIST,
    },
];

const RCH_SQLITE_MIGRATION_SQL: &str = include_str!("../migrations/0001_rch_core_snapshot.sql");
const RCH_SQLITE_MIGRATION_2_SQL: &str = include_str!("../migrations/0002_ordered_migrations.sql");
const RCH_SQLITE_MIGRATION_3_SQL: &str =
    include_str!("../migrations/0003_topic_subscription_corrections.sql");
const RCH_SQLITE_SCHEMA_VERSION: &str = "3";
const RCH_SQLITE_READ_BUSY_TIMEOUT_MS: u64 = 250;
const RCH_SQLITE_WRITE_BUSY_TIMEOUT_MS: u64 = 1_000;
const RCH_SQLITE_ADMIN_BUSY_TIMEOUT_MS: u64 = 30_000;
const RCH_SQLITE_DATA_TABLES: &[&str] = &[
    "rch_topics",
    "rch_subscribers",
    "rch_messages",
    "rch_clients",
    "rch_identity_announces",
    "rch_identity_states",
    "rch_identity_rem_modes",
    "rch_audit_events",
    "rch_system_events",
    "rch_telemetry_records",
    "rch_markers",
    "rch_zones",
    "rch_missions",
    "rch_mission_changes",
    "rch_log_entries",
    "rch_file_attachments",
    "rch_eam_snapshots",
    "rch_teams",
    "rch_mission_team_links",
    "rch_mission_zone_links",
    "rch_mission_marker_links",
    "rch_team_members",
    "rch_team_member_client_links",
    "rch_assets",
    "rch_skills",
    "rch_team_member_skills",
    "rch_task_skill_requirements",
    "rch_assignments",
    "rch_assignment_asset_links",
    "rch_checklists",
    "rch_checklist_templates",
    "rch_checklist_columns",
    "rch_checklist_tasks",
    "rch_checklist_cells",
    "rch_checklist_feed_publications",
    "rch_command_results",
    "rch_identity_capabilities",
    "rch_mission_access_assignments",
    "rch_subject_operation_rights",
    "rch_domain_events",
    "rch_outbound_jobs",
    "rch_settings",
];

#[derive(Debug, Error)]
pub enum RchCoreError {
    #[error("delivery contract violation: {0}")]
    Delivery(String),
    #[error("invalid command payload: {0}")]
    InvalidPayload(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("topic not found")]
    TopicNotFound,
    #[error("mission not found")]
    MissionNotFound,
    #[error("team not found")]
    TeamNotFound,
    #[error("team member not found")]
    TeamMemberNotFound,
    #[error("asset not found")]
    AssetNotFound,
    #[error("skill not found")]
    SkillNotFound,
    #[error("assignment not found")]
    AssignmentNotFound,
    #[error("emergency action message not found")]
    EamNotFound,
    #[error("unsupported command: {0}")]
    UnsupportedCommand(String),
    #[error("RCH state encode failed: {0}")]
    Encode(String),
    #[error("RCH state decode failed: {0}")]
    Decode(String),
    #[error("sqlite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

impl RchCoreError {
    #[must_use]
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::Delivery(_) | Self::InvalidPayload(_) => "invalid_payload",
            Self::Forbidden(_) => "forbidden",
            Self::TopicNotFound
            | Self::MissionNotFound
            | Self::TeamNotFound
            | Self::TeamMemberNotFound
            | Self::AssetNotFound
            | Self::SkillNotFound
            | Self::AssignmentNotFound
            | Self::EamNotFound => "not_found",
            Self::UnsupportedCommand(_) => "unknown_command",
            Self::Encode(_) | Self::Decode(_) | Self::Sqlite(_) => "internal_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildDeliveryEnvelope {
    pub sender: String,
    pub message_id: Option<String>,
    pub topic_id: Option<String>,
    pub content_type: String,
    pub ttl_seconds: u32,
    pub priority: i32,
    pub born_at_ms: Option<i64>,
    pub created_at: Option<String>,
}

impl BuildDeliveryEnvelope {
    #[must_use]
    pub fn new(sender: impl Into<String>) -> Self {
        Self {
            sender: sender.into(),
            message_id: None,
            topic_id: None,
            content_type: "text/plain; schema=lxmf.chat.v1".to_string(),
            ttl_seconds: DEFAULT_TTL_SECONDS,
            priority: DEFAULT_PRIORITY,
            born_at_ms: None,
            created_at: None,
        }
    }
}

pub fn build_delivery_envelope(
    request: BuildDeliveryEnvelope,
) -> Result<DeliveryEnvelope, RchCoreError> {
    let now_ms = utc_now_ms();
    let envelope = DeliveryEnvelope {
        message_id: normalize_message_id(request.message_id.as_deref()),
        content_type: request.content_type.trim().to_ascii_lowercase(),
        schema_version: DELIVERY_SCHEMA_VERSION.to_string(),
        ttl_seconds: request.ttl_seconds,
        priority: request.priority,
        sender: normalize_hash(Some(&request.sender))
            .ok_or_else(|| RchCoreError::Delivery("Sender is required".to_string()))?,
        born_at_ms: request.born_at_ms.unwrap_or(now_ms),
        created_at: Some(request.created_at.unwrap_or_else(utc_now_rfc3339)),
        topic_id: normalize_topic_id(request.topic_id.as_deref()),
    };
    validate_delivery_envelope(&envelope.to_json(), now_ms)?;
    Ok(envelope)
}

pub fn validate_delivery_envelope(
    payload: &Value,
    now_ms: i64,
) -> Result<DeliveryEnvelope, RchCoreError> {
    let mut envelope = r3akt_shared_mesh_delivery::validate_delivery_envelope(payload, now_ms)
        .map_err(|error| shared_delivery_error(&error))?;
    envelope.message_id = normalize_message_id(Some(&envelope.message_id));
    envelope.topic_id = envelope
        .topic_id
        .as_deref()
        .and_then(|topic_id| normalize_topic_id(Some(topic_id)));
    Ok(envelope)
}

pub fn classify_delivery_mode(
    topic_id: Option<&str>,
    destination: Option<&str>,
) -> Result<DeliveryMode, RchCoreError> {
    r3akt_shared_mesh_delivery::classify_delivery_mode(topic_id, destination)
        .map_err(|error| shared_delivery_error(&error))
}

fn shared_delivery_error(error: &r3akt_shared_mesh_delivery::MeshDeliveryError) -> RchCoreError {
    RchCoreError::Delivery(error.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionPolicy {
    Ephemeral,
    Persistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicRecord {
    pub topic_id: String,
    pub topic_name: String,
    pub topic_path: String,
    #[serde(default)]
    pub topic_description: String,
    pub retention: RetentionPolicy,
    pub visibility: Visibility,
    pub created_ts_ms: i64,
    pub last_activity_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriberRecord {
    pub node_id: String,
    pub topic_id: String,
    pub first_seen_ts_ms: i64,
    pub last_seen_ts_ms: i64,
    #[serde(default)]
    pub reject_tests: Option<i64>,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageRecord {
    pub message_id: String,
    pub topic_id: Option<String>,
    pub destination: Option<String>,
    pub sender: String,
    pub content: String,
    pub delivery_mode: DeliveryMode,
    #[serde(default)]
    pub delivery_method: String,
    #[serde(default)]
    pub delivery_policy_reason: String,
    #[serde(default)]
    pub delivery_state: String,
    #[serde(default)]
    pub delivery_metadata: Value,
    pub created_ts_ms: i64,
    #[serde(default)]
    pub attachments: Vec<ChatAttachmentRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRecord {
    pub identity: String,
    pub first_seen_ts_ms: i64,
    pub last_seen_ts_ms: i64,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default = "default_roster_role")]
    pub role: String,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub text_only: bool,
    #[serde(default)]
    pub last_chat_ts_ms: Option<i64>,
}

fn default_roster_role() -> String {
    DEFAULT_ROSTER_ROLE.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityAnnounceRecord {
    pub destination_hash: String,
    #[serde(default)]
    pub announced_identity_hash: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub source_interface: Option<String>,
    #[serde(default)]
    pub announce_capabilities: Vec<String>,
    pub client_type: String,
    pub first_seen_ts_ms: i64,
    pub last_seen_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityStateRecord {
    pub identity: String,
    pub is_banned: bool,
    pub is_blackholed: bool,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityRemModeRecord {
    pub identity: String,
    pub mode: String,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionAuditEvent {
    pub event_id: String,
    pub event_type: String,
    #[serde(default)]
    pub command_type: String,
    pub command_id: String,
    pub source_identity: String,
    pub timestamp_ms: i64,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemEventRecord {
    pub event_id: String,
    pub event_type: String,
    pub message: String,
    pub timestamp_ms: i64,
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryRecord {
    pub peer_destination: String,
    pub timestamp_s: i64,
    pub telemetry: Value,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub identity_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkerRecord {
    pub local_id: String,
    pub object_destination_hash: String,
    pub origin_rch: String,
    pub marker_type: String,
    pub symbol: String,
    pub name: String,
    pub category: String,
    pub lat: f64,
    pub lon: f64,
    pub notes: Option<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZonePointRecord {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneRecord {
    pub zone_id: String,
    pub name: String,
    pub points: Vec<ZonePointRecord>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionRecord {
    pub uid: String,
    pub mission_name: String,
    pub description: String,
    pub topic_id: Option<String>,
    pub path: Option<String>,
    pub classification: Option<String>,
    pub tool: Option<String>,
    pub keywords: Vec<String>,
    pub parent_uid: Option<String>,
    pub feeds: Vec<String>,
    pub password_hash: Option<String>,
    pub default_role: Option<String>,
    pub mission_priority: Option<i64>,
    pub mission_status: String,
    pub owner_role: Option<String>,
    pub token: Option<String>,
    pub invite_only: bool,
    pub expiration: Option<String>,
    pub mission_rde_role: Option<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionZoneLinkRecord {
    pub mission_uid: String,
    pub zone_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionMarkerLinkRecord {
    pub mission_uid: String,
    pub marker_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionChangeRecord {
    pub uid: String,
    pub mission_uid: String,
    pub name: Option<String>,
    pub team_member_rns_identity: Option<String>,
    pub timestamp_ms: i64,
    pub notes: Option<String>,
    pub change_type: String,
    pub is_federated_change: bool,
    pub hashes: Vec<String>,
    pub delta: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntryRecord {
    pub entry_uid: String,
    pub mission_uid: String,
    pub callsign: Option<String>,
    pub content: String,
    pub server_time_ms: i64,
    pub client_time: Option<String>,
    pub content_hashes: Vec<String>,
    pub keywords: Vec<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAttachmentRecord {
    pub file_id: u64,
    pub name: String,
    pub path: String,
    pub category: String,
    pub size: u64,
    pub media_type: Option<String>,
    pub topic_id: Option<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatAttachmentRecord {
    pub file_id: u64,
    pub category: String,
    pub name: String,
    pub size: u64,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EamSnapshotRecord {
    pub eam_uid: String,
    pub callsign: String,
    pub group_name: Option<String>,
    pub team_member_uid: String,
    pub team_uid: String,
    pub reported_by: Option<String>,
    pub reported_ts_ms: i64,
    pub overall_status: String,
    pub security_status: String,
    pub capability_status: String,
    pub preparedness_status: String,
    pub medical_status: String,
    pub mobility_status: String,
    pub comms_status: String,
    pub notes: Option<String>,
    pub confidence: Option<f64>,
    pub ttl_seconds: Option<i64>,
    pub source: Option<Value>,
    pub updated_ts_ms: i64,
    pub deleted_ts_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRecord {
    pub uid: String,
    pub mission_uid: Option<String>,
    pub mission_uids: Vec<String>,
    pub color: Option<String>,
    pub team_name: String,
    pub team_description: String,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamMemberRecord {
    pub uid: String,
    pub team_uid: Option<String>,
    pub rns_identity: String,
    pub display_name: String,
    pub icon: Option<String>,
    pub role: Option<String>,
    pub callsign: Option<String>,
    pub freq: Option<f64>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub modulation: Option<String>,
    pub availability: Option<String>,
    pub certifications: Vec<String>,
    pub last_active: Option<String>,
    pub client_identities: Vec<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRecord {
    pub asset_uid: String,
    pub team_member_uid: Option<String>,
    pub name: String,
    pub asset_type: String,
    pub serial_number: Option<String>,
    pub status: String,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRecord {
    pub skill_uid: String,
    pub name: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub proficiency_scale: Option<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMemberSkillRecord {
    pub uid: String,
    pub team_member_rns_identity: String,
    pub skill_uid: String,
    pub level: i64,
    pub validated_by: Option<String>,
    pub validated_at: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSkillRequirementRecord {
    pub uid: String,
    pub task_uid: String,
    pub skill_uid: String,
    pub minimum_level: i64,
    pub is_mandatory: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentRecord {
    pub assignment_uid: String,
    pub mission_uid: String,
    pub task_uid: String,
    pub team_member_rns_identity: String,
    pub assigned_by: Option<String>,
    pub assigned_ts_ms: i64,
    pub due_dtg: Option<String>,
    pub status: String,
    pub notes: Option<String>,
    pub assets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentAssetLinkRecord {
    pub assignment_uid: String,
    pub asset_uid: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecklistRecord {
    pub uid: String,
    pub mission_uid: Option<String>,
    pub template_uid: Option<String>,
    pub template_version: Option<i64>,
    pub template_name: Option<String>,
    pub name: String,
    pub description: String,
    pub start_ts_ms: i64,
    pub mode: String,
    pub sync_state: String,
    pub origin_type: String,
    pub checklist_status: String,
    pub created_by_team_member_rns_identity: String,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
    pub uploaded_ts_ms: Option<i64>,
    pub progress_percent: f64,
    pub pending_count: i64,
    pub late_count: i64,
    pub complete_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecklistColumnRecord {
    pub column_uid: String,
    pub checklist_uid: Option<String>,
    pub template_uid: Option<String>,
    pub column_name: String,
    pub display_order: i64,
    pub column_type: String,
    pub column_editable: bool,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
    pub is_removable: bool,
    pub system_key: Option<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecklistTemplateRecord {
    pub uid: String,
    pub template_name: String,
    pub description: String,
    pub created_by_team_member_rns_identity: String,
    pub source_template_uid: Option<String>,
    pub server_only: bool,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecklistTaskRecord {
    pub task_uid: String,
    pub checklist_uid: String,
    pub number: i64,
    pub user_status: String,
    pub task_status: String,
    pub is_late: bool,
    pub custom_status: Option<String>,
    pub due_relative_minutes: Option<i64>,
    pub due_ts_ms: Option<i64>,
    pub notes: Option<String>,
    pub row_background_color: Option<String>,
    pub line_break_enabled: bool,
    pub completed_ts_ms: Option<i64>,
    pub completed_by_team_member_rns_identity: Option<String>,
    pub legacy_value: Option<String>,
    pub created_ts_ms: i64,
    pub updated_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecklistCellRecord {
    pub cell_uid: String,
    pub task_uid: String,
    pub column_uid: String,
    pub value: Option<String>,
    pub updated_ts_ms: i64,
    pub updated_by_team_member_rns_identity: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistFeedPublicationRecord {
    pub publication_uid: String,
    pub checklist_uid: String,
    pub mission_feed_uid: String,
    pub published_ts_ms: i64,
    pub published_by_team_member_rns_identity: String,
}

type ChecklistSnapshotRows = (
    Vec<ChecklistRecord>,
    Vec<ChecklistTemplateRecord>,
    Vec<ChecklistColumnRecord>,
    Vec<ChecklistTaskRecord>,
    Vec<ChecklistCellRecord>,
    Vec<ChecklistFeedPublicationRecord>,
);

type MissionTeamRecipientRows = (
    Vec<MissionTeamLinkRecord>,
    Vec<TeamMemberRecord>,
    Vec<TeamMemberClientLinkRecord>,
);

#[derive(Debug, Clone, PartialEq)]
pub struct RchCommandOutcome {
    pub result: CommandResultEnvelope,
    pub event: Option<EventEnvelope>,
}

#[derive(Debug, Default)]
pub struct RchCore {
    topics: HashMap<String, TopicRecord>,
    subscribers: HashMap<(String, String), SubscriberRecord>,
    messages: Vec<MessageRecord>,
    clients: HashMap<String, ClientRecord>,
    identity_announces: HashMap<String, IdentityAnnounceRecord>,
    identity_states: HashMap<String, IdentityStateRecord>,
    identity_rem_modes: HashMap<String, IdentityRemModeRecord>,
    audit_events: Vec<MissionAuditEvent>,
    system_events: Vec<SystemEventRecord>,
    telemetry_records: Vec<TelemetryRecord>,
    markers: HashMap<String, MarkerRecord>,
    zones: HashMap<String, ZoneRecord>,
    missions: HashMap<String, MissionRecord>,
    mission_changes: HashMap<String, MissionChangeRecord>,
    log_entries: HashMap<String, LogEntryRecord>,
    file_attachments: HashMap<u64, FileAttachmentRecord>,
    eam_snapshots: HashMap<String, EamSnapshotRecord>,
    teams: HashMap<String, TeamRecord>,
    mission_team_links: HashSet<(String, String)>,
    mission_zone_links: HashSet<(String, String)>,
    mission_marker_links: HashSet<(String, String)>,
    team_members: HashMap<String, TeamMemberRecord>,
    team_member_client_links: HashSet<(String, String)>,
    assets: HashMap<String, AssetRecord>,
    skills: HashMap<String, SkillRecord>,
    team_member_skills: HashMap<String, TeamMemberSkillRecord>,
    task_skill_requirements: HashMap<String, TaskSkillRequirementRecord>,
    assignments: HashMap<String, AssignmentRecord>,
    assignment_asset_links: HashSet<(String, String)>,
    checklists: HashMap<String, ChecklistRecord>,
    checklist_templates: HashMap<String, ChecklistTemplateRecord>,
    checklist_columns: HashMap<String, ChecklistColumnRecord>,
    checklist_tasks: HashMap<String, ChecklistTaskRecord>,
    checklist_cells: HashMap<String, ChecklistCellRecord>,
    checklist_feed_publications: HashMap<String, ChecklistFeedPublicationRecord>,
    command_results: HashMap<String, CommandResultEnvelope>,
    identity_capabilities: HashMap<String, HashSet<String>>,
    mission_access_assignments: HashMap<(String, String, String), MissionAccessAssignment>,
    subject_operation_rights:
        HashMap<(String, String, String, String, String), SubjectOperationRight>,
    authorization_required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RchCoreSnapshot {
    pub topics: Vec<TopicRecord>,
    pub subscribers: Vec<SubscriberRecord>,
    pub messages: Vec<MessageRecord>,
    #[serde(default)]
    pub clients: Vec<ClientRecord>,
    #[serde(default)]
    pub identity_announces: Vec<IdentityAnnounceRecord>,
    #[serde(default)]
    pub identity_states: Vec<IdentityStateRecord>,
    #[serde(default)]
    pub identity_rem_modes: Vec<IdentityRemModeRecord>,
    #[serde(default)]
    pub audit_events: Vec<MissionAuditEvent>,
    #[serde(default)]
    pub system_events: Vec<SystemEventRecord>,
    #[serde(default)]
    pub telemetry_records: Vec<TelemetryRecord>,
    #[serde(default)]
    pub markers: Vec<MarkerRecord>,
    #[serde(default)]
    pub zones: Vec<ZoneRecord>,
    #[serde(default)]
    pub missions: Vec<MissionRecord>,
    #[serde(default)]
    pub mission_changes: Vec<MissionChangeRecord>,
    #[serde(default)]
    pub log_entries: Vec<LogEntryRecord>,
    #[serde(default)]
    pub file_attachments: Vec<FileAttachmentRecord>,
    #[serde(default)]
    pub eam_snapshots: Vec<EamSnapshotRecord>,
    #[serde(default)]
    pub teams: Vec<TeamRecord>,
    #[serde(default)]
    pub mission_team_links: Vec<MissionTeamLinkRecord>,
    #[serde(default)]
    pub mission_zone_links: Vec<MissionZoneLinkRecord>,
    #[serde(default)]
    pub mission_marker_links: Vec<MissionMarkerLinkRecord>,
    #[serde(default)]
    pub team_members: Vec<TeamMemberRecord>,
    #[serde(default)]
    pub team_member_client_links: Vec<TeamMemberClientLinkRecord>,
    #[serde(default)]
    pub assets: Vec<AssetRecord>,
    #[serde(default)]
    pub skills: Vec<SkillRecord>,
    #[serde(default)]
    pub team_member_skills: Vec<TeamMemberSkillRecord>,
    #[serde(default)]
    pub task_skill_requirements: Vec<TaskSkillRequirementRecord>,
    #[serde(default)]
    pub assignments: Vec<AssignmentRecord>,
    #[serde(default)]
    pub assignment_asset_links: Vec<AssignmentAssetLinkRecord>,
    #[serde(default)]
    pub checklists: Vec<ChecklistRecord>,
    #[serde(default)]
    pub checklist_templates: Vec<ChecklistTemplateRecord>,
    #[serde(default)]
    pub checklist_columns: Vec<ChecklistColumnRecord>,
    #[serde(default)]
    pub checklist_tasks: Vec<ChecklistTaskRecord>,
    #[serde(default)]
    pub checklist_cells: Vec<ChecklistCellRecord>,
    #[serde(default)]
    pub checklist_feed_publications: Vec<ChecklistFeedPublicationRecord>,
    pub command_results: Vec<CommandResultEnvelope>,
    #[serde(default)]
    pub identity_capabilities: Vec<IdentityCapabilityGrant>,
    #[serde(default)]
    pub mission_access_assignments: Vec<MissionAccessAssignment>,
    #[serde(default)]
    pub subject_operation_rights: Vec<SubjectOperationRight>,
    #[serde(default)]
    pub authorization_required: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct SnapshotSaveOptions {
    preserve_identity_announces: bool,
}

#[derive(Debug, Clone)]
struct MessageQueueProjection {
    delivery_state: String,
    dispatch_status: String,
    next_attempt_at_ts_ms: Option<i64>,
    attempts: i64,
    priority: i64,
    batch_id: Option<String>,
    created_ts_ms: i64,
}

fn message_queue_projection(record: &MessageRecord) -> MessageQueueProjection {
    MessageQueueProjection {
        delivery_state: if record.delivery_state.trim().is_empty() {
            "queued".to_string()
        } else {
            record.delivery_state.clone()
        },
        dispatch_status: record
            .delivery_metadata
            .get("dispatch_status")
            .and_then(Value::as_str)
            .map_or_else(|| "queued".to_string(), str::to_string),
        next_attempt_at_ts_ms: record
            .delivery_metadata
            .get("next_attempt_at_ts_ms")
            .and_then(Value::as_i64),
        attempts: record
            .delivery_metadata
            .get("attempts")
            .and_then(Value::as_i64)
            .or_else(|| {
                record
                    .delivery_metadata
                    .get("attempts")
                    .and_then(Value::as_u64)
                    .and_then(|value| i64::try_from(value).ok())
            })
            .unwrap_or(0),
        priority: record
            .delivery_metadata
            .get("priority")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        batch_id: record
            .delivery_metadata
            .get("batch_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        created_ts_ms: record.created_ts_ms,
    }
}

impl RchCoreSnapshot {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.topics.is_empty()
            && self.subscribers.is_empty()
            && self.messages.is_empty()
            && self.clients.is_empty()
            && self.identity_announces.is_empty()
            && self.identity_states.is_empty()
            && self.identity_rem_modes.is_empty()
            && self.audit_events.is_empty()
            && self.system_events.is_empty()
            && self.telemetry_records.is_empty()
            && self.markers.is_empty()
            && self.zones.is_empty()
            && self.missions.is_empty()
            && self.mission_changes.is_empty()
            && self.log_entries.is_empty()
            && self.file_attachments.is_empty()
            && self.eam_snapshots.is_empty()
            && self.teams.is_empty()
            && self.mission_team_links.is_empty()
            && self.mission_zone_links.is_empty()
            && self.mission_marker_links.is_empty()
            && self.team_members.is_empty()
            && self.team_member_client_links.is_empty()
            && self.assets.is_empty()
            && self.skills.is_empty()
            && self.team_member_skills.is_empty()
            && self.task_skill_requirements.is_empty()
            && self.assignments.is_empty()
            && self.assignment_asset_links.is_empty()
            && self.checklists.is_empty()
            && self.checklist_templates.is_empty()
            && self.checklist_columns.is_empty()
            && self.checklist_tasks.is_empty()
            && self.checklist_cells.is_empty()
            && self.checklist_feed_publications.is_empty()
            && self.command_results.is_empty()
            && self.identity_capabilities.is_empty()
            && self.mission_access_assignments.is_empty()
            && self.subject_operation_rights.is_empty()
            && !self.authorization_required
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityCapabilityGrant {
    pub identity: String,
    pub capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionAccessAssignment {
    pub mission_uid: String,
    pub subject_type: String,
    pub subject_id: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectOperationRight {
    pub grant_uid: String,
    pub subject_type: String,
    pub subject_id: String,
    pub operation: String,
    pub scope_type: String,
    pub scope_id: String,
    pub granted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionTeamLinkRecord {
    pub mission_uid: String,
    pub team_uid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMemberClientLinkRecord {
    pub team_member_uid: String,
    pub client_identity: String,
}

#[derive(Debug)]
pub struct RchSqliteStore {
    connection: Connection,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SqliteStorageStats {
    pub page_count: u64,
    pub free_page_count: u64,
    pub page_size: u64,
    pub free_percent: f64,
    pub auto_vacuum: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SqliteCompactionReport {
    pub before: SqliteStorageStats,
    pub after: SqliteStorageStats,
    pub compacted: bool,
    pub requires_incremental_mode_conversion: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RchDatabaseWipeTableReport {
    pub table: String,
    pub rows_deleted: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RchDatabaseWipeReport {
    pub tables: Vec<RchDatabaseWipeTableReport>,
    pub rows_deleted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RchSqliteConnectionProfile {
    ReadOnly,
    Write,
    Admin,
}

impl RchSqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RchCoreError> {
        let connection = Connection::open(path)?;
        Self::from_connection_with_profile(connection, RchSqliteConnectionProfile::Write)
    }

    pub fn open_admin(path: impl AsRef<Path>) -> Result<Self, RchCoreError> {
        let connection = Connection::open(path)?;
        Self::from_connection_with_profile(connection, RchSqliteConnectionProfile::Admin)
    }

    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, RchCoreError> {
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        configure_sqlite_connection(&connection, RchSqliteConnectionProfile::ReadOnly)?;
        Ok(Self { connection })
    }

    pub fn in_memory() -> Result<Self, RchCoreError> {
        Self::from_connection_with_profile(
            Connection::open_in_memory()?,
            RchSqliteConnectionProfile::Admin,
        )
    }

    pub fn from_connection(connection: Connection) -> Result<Self, RchCoreError> {
        Self::from_connection_with_profile(connection, RchSqliteConnectionProfile::Admin)
    }

    fn from_connection_with_profile(
        connection: Connection,
        profile: RchSqliteConnectionProfile,
    ) -> Result<Self, RchCoreError> {
        configure_sqlite_connection(&connection, profile)?;
        let store = Self { connection };
        if profile != RchSqliteConnectionProfile::ReadOnly {
            store.migrate()?;
        }
        Ok(store)
    }

    pub fn save_snapshot(&mut self, snapshot: &RchCoreSnapshot) -> Result<(), RchCoreError> {
        self.save_snapshot_with_options(snapshot, SnapshotSaveOptions::default())
    }

    pub fn save_snapshot_preserving_identity_announces(
        &mut self,
        snapshot: &RchCoreSnapshot,
    ) -> Result<(), RchCoreError> {
        self.save_snapshot_with_options(
            snapshot,
            SnapshotSaveOptions {
                preserve_identity_announces: true,
            },
        )
    }

    pub fn save_checklist_command_projection(
        &mut self,
        snapshot: &RchCoreSnapshot,
    ) -> Result<(), RchCoreError> {
        let transaction = self.connection.transaction()?;
        for table in [
            "rch_checklists",
            "rch_checklist_templates",
            "rch_checklist_columns",
            "rch_checklist_tasks",
            "rch_checklist_cells",
            "rch_checklist_feed_publications",
            "rch_task_skill_requirements",
            "rch_assignments",
            "rch_assignment_asset_links",
        ] {
            transaction.execute(&format!("DELETE FROM {table}"), [])?;
        }
        save_checklist_snapshot_tables(&transaction, snapshot)?;
        for record in &snapshot.task_skill_requirements {
            transaction.execute(
                "INSERT INTO rch_task_skill_requirements (task_uid, skill_uid, payload)
                 VALUES (?1, ?2, ?3)",
                params![record.task_uid, record.skill_uid, encode_msgpack(record)?],
            )?;
        }
        for assignment in &snapshot.assignments {
            transaction.execute(
                "INSERT INTO rch_assignments
                    (assignment_uid, mission_uid, task_uid, team_member_rns_identity, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    assignment.assignment_uid,
                    assignment.mission_uid,
                    assignment.task_uid,
                    assignment.team_member_rns_identity,
                    encode_msgpack(assignment)?
                ],
            )?;
        }
        for link in &snapshot.assignment_asset_links {
            transaction.execute(
                "INSERT INTO rch_assignment_asset_links (assignment_uid, asset_uid, payload)
                 VALUES (?1, ?2, ?3)",
                params![link.assignment_uid, link.asset_uid, encode_msgpack(link)?],
            )?;
        }
        for change in &snapshot.mission_changes {
            transaction.execute(
                "INSERT OR REPLACE INTO rch_mission_changes (uid, mission_uid, payload)
                 VALUES (?1, ?2, ?3)",
                params![change.uid, change.mission_uid, encode_msgpack(change)?],
            )?;
        }
        for result in &snapshot.command_results {
            transaction.execute(
                "INSERT OR REPLACE INTO rch_command_results (command_id, payload) VALUES (?1, ?2)",
                params![result.command_id, encode_msgpack(result)?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn save_checklist_command_delta(
        &mut self,
        before: &RchCoreSnapshot,
        after: &RchCoreSnapshot,
    ) -> Result<(), RchCoreError> {
        let transaction = self.connection.transaction()?;
        save_checklist_delta_tables(&transaction, before, after)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn save_r3akt_command_delta(
        &mut self,
        before: &RchCoreSnapshot,
        after: &RchCoreSnapshot,
    ) -> Result<(), RchCoreError> {
        let transaction = self.connection.transaction()?;
        save_registry_delta_tables(&transaction, before, after)?;
        transaction.commit()?;
        Ok(())
    }

    fn save_snapshot_with_options(
        &mut self,
        snapshot: &RchCoreSnapshot,
        options: SnapshotSaveOptions,
    ) -> Result<(), RchCoreError> {
        let preserved_settings = self.settings_with_prefix("reticulumd_")?;
        let transaction = self.connection.transaction()?;
        clear_snapshot_tables(&transaction, options)?;
        save_topic_snapshot_tables(&transaction, snapshot, options)?;
        save_registry_snapshot_tables(&transaction, snapshot)?;
        for result in &snapshot.command_results {
            transaction.execute(
                "INSERT INTO rch_command_results (command_id, payload) VALUES (?1, ?2)",
                params![result.command_id, encode_msgpack(result)?],
            )?;
        }
        for grant in &snapshot.identity_capabilities {
            transaction.execute(
                "INSERT INTO rch_identity_capabilities (identity, capability, payload)
                 VALUES (?1, ?2, ?3)",
                params![grant.identity, grant.capability, encode_msgpack(grant)?],
            )?;
        }
        for assignment in &snapshot.mission_access_assignments {
            transaction.execute(
                "INSERT INTO rch_mission_access_assignments (mission_uid, subject_type, subject_id, payload)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    assignment.mission_uid,
                    assignment.subject_type,
                    assignment.subject_id,
                    encode_msgpack(assignment)?
                ],
            )?;
        }
        for right in &snapshot.subject_operation_rights {
            transaction.execute(
                "INSERT INTO rch_subject_operation_rights
                    (subject_type, subject_id, operation, scope_type, scope_id, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    right.subject_type,
                    right.subject_id,
                    right.operation,
                    right.scope_type,
                    right.scope_id,
                    encode_msgpack(right)?,
                ],
            )?;
        }
        transaction.execute(
            "INSERT INTO rch_settings (setting_key, setting_value)
             VALUES ('schema_version', ?1)",
            params![RCH_SQLITE_SCHEMA_VERSION],
        )?;
        transaction.execute(
            "INSERT INTO rch_settings (setting_key, setting_value)
             VALUES ('authorization_required', ?1)",
            params![if snapshot.authorization_required {
                "true"
            } else {
                "false"
            }],
        )?;
        for (key, value) in preserved_settings {
            transaction.execute(
                "INSERT OR REPLACE INTO rch_settings (setting_key, setting_value)
                 VALUES (?1, ?2)",
                params![key, value],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<String, RchCoreError> {
        Ok(self
            .setting_value("schema_version")?
            .unwrap_or_else(|| RCH_SQLITE_SCHEMA_VERSION.to_string()))
    }

    pub fn optimize(&self) -> Result<(), RchCoreError> {
        self.connection.execute_batch("PRAGMA optimize;")?;
        Ok(())
    }

    pub fn integrity_check(&self) -> Result<(), RchCoreError> {
        let result: String = self
            .connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if result == "ok" {
            Ok(())
        } else {
            Err(RchCoreError::InvalidPayload(format!(
                "SQLite integrity check failed: {result}"
            )))
        }
    }

    pub fn storage_stats(&self) -> Result<SqliteStorageStats, RchCoreError> {
        let page_count = sqlite_pragma_u64(&self.connection, "page_count")?;
        let free_page_count = sqlite_pragma_u64(&self.connection, "freelist_count")?;
        let page_size = sqlite_pragma_u64(&self.connection, "page_size")?;
        let auto_vacuum = self
            .connection
            .query_row("PRAGMA auto_vacuum", [], |row| row.get::<_, i64>(0))?;
        let free_percent = if page_count == 0 {
            0.0
        } else {
            let basis_points =
                u128::from(free_page_count).saturating_mul(10_000) / u128::from(page_count);
            f64::from(u32::try_from(basis_points).unwrap_or(10_000)) / 100.0
        };
        Ok(SqliteStorageStats {
            page_count,
            free_page_count,
            page_size,
            free_percent,
            auto_vacuum,
        })
    }

    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<(), RchCoreError> {
        let mut destination = Connection::open(path)?;
        let backup = rusqlite::backup::Backup::new(&self.connection, &mut destination)?;
        backup.run_to_completion(128, Duration::from_millis(10), None)?;
        Ok(())
    }

    pub fn enable_incremental_vacuum_with_backup(
        &self,
        backup_path: impl AsRef<Path>,
    ) -> Result<SqliteStorageStats, RchCoreError> {
        self.integrity_check()?;
        self.backup_to(backup_path)?;
        self.connection
            .execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM; PRAGMA optimize;")?;
        self.integrity_check()?;
        self.storage_stats()
    }

    pub fn compact_if_free_percent_exceeds(
        &self,
        threshold_percent: f64,
        max_pages: u64,
    ) -> Result<SqliteCompactionReport, RchCoreError> {
        let before = self.storage_stats()?;
        let threshold_percent = threshold_percent.clamp(0.0, 100.0);
        if before.free_percent <= threshold_percent || before.free_page_count == 0 {
            return Ok(SqliteCompactionReport {
                before,
                after: before,
                compacted: false,
                requires_incremental_mode_conversion: false,
            });
        }
        // SQLite reports 2 for INCREMENTAL. Existing databases must be
        // converted with the explicit backup + full VACUUM operation above.
        if before.auto_vacuum != 2 {
            return Ok(SqliteCompactionReport {
                before,
                after: before,
                compacted: false,
                requires_incremental_mode_conversion: true,
            });
        }
        let pages = before.free_page_count.min(max_pages.max(1));
        self.connection.execute_batch(&format!(
            "PRAGMA incremental_vacuum({pages}); PRAGMA optimize;"
        ))?;
        let after = self.storage_stats()?;
        Ok(SqliteCompactionReport {
            before,
            after,
            compacted: after.free_page_count < before.free_page_count,
            requires_incremental_mode_conversion: false,
        })
    }

    pub fn upsert_identity_announces(
        &mut self,
        records: &[IdentityAnnounceRecord],
    ) -> Result<(), RchCoreError> {
        if records.is_empty() {
            return Ok(());
        }
        let transaction = self.connection.transaction()?;
        for record in records {
            transaction.execute(
                "INSERT OR REPLACE INTO rch_identity_announces (destination_hash, payload)
                 VALUES (?1, ?2)",
                params![record.destination_hash, encode_msgpack(record)?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn upsert_message(&mut self, record: &MessageRecord) -> Result<(), RchCoreError> {
        let projection = message_queue_projection(record);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM rch_messages WHERE message_id = ?1",
            params![record.message_id],
        )?;
        transaction.execute(
            "INSERT INTO rch_messages (
                message_id,
                payload,
                delivery_state,
                dispatch_status,
                next_attempt_at_ts_ms,
                attempts,
                priority,
                batch_id,
                created_ts_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.message_id,
                encode_msgpack(record)?,
                projection.delivery_state,
                projection.dispatch_status,
                projection.next_attempt_at_ts_ms,
                projection.attempts,
                projection.priority,
                projection.batch_id,
                projection.created_ts_ms,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn upsert_system_event(&mut self, record: &SystemEventRecord) -> Result<(), RchCoreError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM rch_system_events WHERE event_id = ?1",
            params![record.event_id],
        )?;
        transaction.execute(
            "INSERT INTO rch_system_events (event_id, payload) VALUES (?1, ?2)",
            params![record.event_id, encode_msgpack(record)?],
        )?;
        transaction.execute(
            "DELETE FROM rch_system_events
             WHERE id NOT IN (
                 SELECT id FROM rch_system_events ORDER BY id DESC LIMIT 200
             )",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn insert_telemetry_record(
        &mut self,
        record: &TelemetryRecord,
    ) -> Result<(), RchCoreError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM rch_telemetry_records WHERE lower(peer_destination) = lower(?1)",
            params![record.peer_destination],
        )?;
        transaction.execute(
            "INSERT INTO rch_telemetry_records (peer_destination, timestamp_s, payload)
             VALUES (?1, ?2, ?3)",
            params![
                record.peer_destination,
                record.timestamp_s,
                encode_msgpack(record)?
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn prune_telemetry_records(
        &mut self,
        excluded_peer_destination: Option<&str>,
    ) -> Result<(), RchCoreError> {
        let transaction = self.connection.transaction()?;
        if let Some(destination) = excluded_peer_destination {
            transaction.execute(
                "DELETE FROM rch_telemetry_records WHERE lower(peer_destination) = lower(?1)",
                params![destination],
            )?;
        }
        transaction.execute(
            "DELETE FROM rch_telemetry_records
             WHERE id NOT IN (
                 SELECT keep.id
                 FROM rch_telemetry_records AS keep
                 WHERE keep.id = (
                     SELECT candidate.id
                     FROM rch_telemetry_records AS candidate
                     WHERE lower(candidate.peer_destination) = lower(keep.peer_destination)
                     ORDER BY candidate.timestamp_s DESC, candidate.id DESC
                     LIMIT 1
                 )
             )",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn load_identity_announces(&self) -> Result<Vec<IdentityAnnounceRecord>, RchCoreError> {
        self.load_payload_rows::<IdentityAnnounceRecord>(
            "SELECT payload FROM rch_identity_announces ORDER BY destination_hash",
        )
    }

    pub fn list_messages_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<MessageRecord>, RchCoreError> {
        self.load_payload_page(
            "SELECT payload FROM rch_messages ORDER BY created_ts_ms DESC, id DESC LIMIT ?1 OFFSET ?2",
            limit,
            offset,
        )
    }

    pub fn list_system_events_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SystemEventRecord>, RchCoreError> {
        self.load_payload_page(
            "SELECT payload FROM rch_system_events ORDER BY id DESC LIMIT ?1 OFFSET ?2",
            limit,
            offset,
        )
    }

    pub fn list_telemetry_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<TelemetryRecord>, RchCoreError> {
        self.load_payload_page(
            "SELECT payload FROM rch_telemetry_records ORDER BY timestamp_s DESC, id DESC LIMIT ?1 OFFSET ?2",
            limit,
            offset,
        )
    }

    pub fn load_identity_announces_by_destination_hashes(
        &self,
        destinations: &[String],
    ) -> Result<Vec<IdentityAnnounceRecord>, RchCoreError> {
        if destinations.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM rch_identity_announces WHERE destination_hash = ?1")?;
        let mut seen = HashSet::new();
        let mut records = Vec::new();
        for destination in destinations {
            let destination = destination.trim().to_ascii_lowercase();
            if destination.is_empty() || !seen.insert(destination.clone()) {
                continue;
            }
            let rows = statement.query_map(params![destination], |row| row.get::<_, Vec<u8>>(0))?;
            for row in rows {
                records.push(decode_msgpack(&row?)?);
            }
        }
        Ok(records)
    }

    pub fn load_identity_states(&self) -> Result<Vec<IdentityStateRecord>, RchCoreError> {
        self.load_payload_rows::<IdentityStateRecord>(
            "SELECT payload FROM rch_identity_states ORDER BY identity",
        )
    }

    pub fn load_identity_rem_modes(&self) -> Result<Vec<IdentityRemModeRecord>, RchCoreError> {
        self.load_payload_rows::<IdentityRemModeRecord>(
            "SELECT payload FROM rch_identity_rem_modes ORDER BY identity",
        )
    }

    pub fn load_identity_rem_modes_by_identities(
        &self,
        identities: &[String],
    ) -> Result<Vec<IdentityRemModeRecord>, RchCoreError> {
        if identities.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM rch_identity_rem_modes WHERE identity = ?1")?;
        let mut seen = HashSet::new();
        let mut records = Vec::new();
        for identity in identities {
            let identity = identity.trim().to_ascii_lowercase();
            if identity.is_empty() || !seen.insert(identity.clone()) {
                continue;
            }
            let rows = statement.query_map(params![identity], |row| row.get::<_, Vec<u8>>(0))?;
            for row in rows {
                records.push(decode_msgpack(&row?)?);
            }
        }
        Ok(records)
    }

    pub fn count_file_attachments_by_category(
        &self,
        category: &str,
    ) -> Result<usize, RchCoreError> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM rch_file_attachments WHERE category = ?1",
            params![category],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    pub fn load_file_attachments_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<FileAttachmentRecord>, RchCoreError> {
        let mut statement = self.connection.prepare(
            "SELECT payload FROM rch_file_attachments WHERE category = ?1 ORDER BY file_id",
        )?;
        let rows = statement.query_map(params![category], |row| row.get::<_, Vec<u8>>(0))?;
        let mut records = Vec::new();
        for row in rows {
            records.push(decode_msgpack(&row?)?);
        }
        Ok(records)
    }

    pub fn load_file_attachment(
        &self,
        file_id: u64,
        category: &str,
    ) -> Result<Option<FileAttachmentRecord>, RchCoreError> {
        let mut statement = self.connection.prepare(
            "SELECT payload FROM rch_file_attachments WHERE file_id = ?1 AND category = ?2",
        )?;
        let mut rows = statement.query(params![file_id, category])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let payload = row.get::<_, Vec<u8>>(0)?;
        Ok(Some(decode_msgpack(&payload)?))
    }

    pub fn next_file_attachment_id(&self) -> Result<u64, RchCoreError> {
        let max_id = self.connection.query_row(
            "SELECT COALESCE(MAX(file_id), 0) FROM rch_file_attachments",
            [],
            |row| row.get::<_, u64>(0),
        )?;
        Ok(max_id.saturating_add(1))
    }

    pub fn upsert_file_attachment(
        &mut self,
        record: &FileAttachmentRecord,
    ) -> Result<(), RchCoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO rch_file_attachments (file_id, category, payload)
             VALUES (?1, ?2, ?3)",
            params![record.file_id, record.category, encode_msgpack(record)?],
        )?;
        Ok(())
    }

    pub fn insert_file_attachment_with_next_id(
        &mut self,
        mut record: FileAttachmentRecord,
    ) -> Result<FileAttachmentRecord, RchCoreError> {
        let transaction = self.connection.transaction()?;
        let max_id = transaction.query_row(
            "SELECT COALESCE(MAX(file_id), 0) FROM rch_file_attachments",
            [],
            |row| row.get::<_, u64>(0),
        )?;
        record.file_id = max_id.saturating_add(1);
        transaction.execute(
            "INSERT INTO rch_file_attachments (file_id, category, payload)
             VALUES (?1, ?2, ?3)",
            params![record.file_id, record.category, encode_msgpack(&record)?],
        )?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn delete_file_attachment(
        &mut self,
        file_id: u64,
        category: &str,
    ) -> Result<(), RchCoreError> {
        self.connection.execute(
            "DELETE FROM rch_file_attachments WHERE file_id = ?1 AND category = ?2",
            params![file_id, category],
        )?;
        Ok(())
    }

    pub fn upsert_identity_states(
        &mut self,
        records: &[IdentityStateRecord],
    ) -> Result<(), RchCoreError> {
        if records.is_empty() {
            return Ok(());
        }
        let transaction = self.connection.transaction()?;
        for record in records {
            transaction.execute(
                "INSERT OR REPLACE INTO rch_identity_states (identity, payload)
                 VALUES (?1, ?2)",
                params![record.identity, encode_msgpack(record)?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn upsert_client(&mut self, record: &ClientRecord) -> Result<(), RchCoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO rch_clients (identity, payload) VALUES (?1, ?2)",
            params![record.identity, encode_msgpack(record)?],
        )?;
        Ok(())
    }

    pub fn delete_client(&mut self, identity: &str) -> Result<(), RchCoreError> {
        self.connection.execute(
            "DELETE FROM rch_clients WHERE identity = ?1",
            params![identity],
        )?;
        Ok(())
    }

    pub fn upsert_identity_state(
        &mut self,
        record: &IdentityStateRecord,
    ) -> Result<(), RchCoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO rch_identity_states (identity, payload) VALUES (?1, ?2)",
            params![record.identity, encode_msgpack(record)?],
        )?;
        Ok(())
    }

    pub fn upsert_topic(&mut self, record: &TopicRecord) -> Result<(), RchCoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO rch_topics (topic_id, payload) VALUES (?1, ?2)",
            params![record.topic_id, encode_msgpack(record)?],
        )?;
        Ok(())
    }

    pub fn delete_topic(&mut self, topic_id: &str) -> Result<(), RchCoreError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM rch_topics WHERE topic_id = ?1",
            params![topic_id],
        )?;
        transaction.execute(
            "DELETE FROM rch_subscribers WHERE topic_id = ?1",
            params![topic_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn upsert_subscriber(&mut self, record: &SubscriberRecord) -> Result<(), RchCoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO rch_subscribers (node_id, topic_id, payload)
             VALUES (?1, ?2, ?3)",
            params![record.node_id, record.topic_id, encode_msgpack(record)?],
        )?;
        Ok(())
    }

    pub fn delete_subscriber(
        &mut self,
        subscriber_id: &str,
        topic_id: &str,
    ) -> Result<(), RchCoreError> {
        self.connection.execute(
            "DELETE FROM rch_subscribers WHERE node_id = ?1 AND topic_id = ?2",
            params![subscriber_id, topic_id],
        )?;
        Ok(())
    }

    pub fn upsert_marker(&mut self, record: &MarkerRecord) -> Result<(), RchCoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO rch_markers (object_destination_hash, payload)
             VALUES (?1, ?2)",
            params![record.object_destination_hash, encode_msgpack(record)?],
        )?;
        Ok(())
    }

    pub fn delete_marker(&mut self, object_destination_hash: &str) -> Result<(), RchCoreError> {
        self.connection.execute(
            "DELETE FROM rch_markers WHERE object_destination_hash = ?1",
            params![object_destination_hash],
        )?;
        Ok(())
    }

    pub fn upsert_zone(&mut self, record: &ZoneRecord) -> Result<(), RchCoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO rch_zones (zone_id, payload) VALUES (?1, ?2)",
            params![record.zone_id, encode_msgpack(record)?],
        )?;
        Ok(())
    }

    pub fn delete_zone(&mut self, zone_id: &str) -> Result<(), RchCoreError> {
        self.connection
            .execute("DELETE FROM rch_zones WHERE zone_id = ?1", params![zone_id])?;
        Ok(())
    }

    pub fn load_snapshot(&self) -> Result<Option<RchCoreSnapshot>, RchCoreError> {
        let snapshot = self.load_snapshot_rows(true)?;
        if snapshot.is_empty() {
            Ok(None)
        } else {
            Ok(Some(snapshot))
        }
    }

    pub fn load_snapshot_without_identity_announces(
        &self,
    ) -> Result<Option<RchCoreSnapshot>, RchCoreError> {
        let snapshot = self.load_snapshot_rows(false)?;
        if snapshot.is_empty() {
            Ok(None)
        } else {
            Ok(Some(snapshot))
        }
    }

    pub fn load_checklist_command_snapshot(&self) -> Result<RchCoreSnapshot, RchCoreError> {
        let mut snapshot = RchCore::new().snapshot();
        let (
            checklists,
            checklist_templates,
            checklist_columns,
            checklist_tasks,
            checklist_cells,
            checklist_feed_publications,
        ) = self.load_checklist_snapshot_rows()?;
        snapshot.checklists = checklists;
        snapshot.checklist_templates = checklist_templates;
        snapshot.checklist_columns = checklist_columns;
        snapshot.checklist_tasks = checklist_tasks;
        snapshot.checklist_cells = checklist_cells;
        snapshot.checklist_feed_publications = checklist_feed_publications;
        snapshot.missions = self
            .load_payload_rows::<MissionRecord>("SELECT payload FROM rch_missions ORDER BY uid")?;
        snapshot.teams =
            self.load_payload_rows::<TeamRecord>("SELECT payload FROM rch_teams ORDER BY uid")?;
        snapshot.mission_team_links = self.load_payload_rows::<MissionTeamLinkRecord>(
            "SELECT payload FROM rch_mission_team_links ORDER BY mission_uid, team_uid",
        )?;
        snapshot.team_members = self.load_payload_rows::<TeamMemberRecord>(
            "SELECT payload FROM rch_team_members ORDER BY uid",
        )?;
        snapshot.team_member_client_links = self.load_payload_rows::<TeamMemberClientLinkRecord>(
            "SELECT payload FROM rch_team_member_client_links ORDER BY team_member_uid, client_identity",
        )?;
        snapshot.mission_changes = self.load_payload_rows::<MissionChangeRecord>(
            "SELECT payload FROM rch_mission_changes ORDER BY uid",
        )?;
        snapshot.task_skill_requirements = self.load_payload_rows::<TaskSkillRequirementRecord>(
            "SELECT payload FROM rch_task_skill_requirements ORDER BY task_uid, skill_uid",
        )?;
        snapshot.assignments = self.load_payload_rows::<AssignmentRecord>(
            "SELECT payload FROM rch_assignments ORDER BY assignment_uid",
        )?;
        snapshot.assignment_asset_links = self.load_payload_rows::<AssignmentAssetLinkRecord>(
            "SELECT payload FROM rch_assignment_asset_links ORDER BY assignment_uid, asset_uid",
        )?;
        Ok(snapshot)
    }

    pub fn load_mission_team_recipient_rows(
        &self,
    ) -> Result<MissionTeamRecipientRows, RchCoreError> {
        Ok((
            self.load_payload_rows::<MissionTeamLinkRecord>(
                "SELECT payload FROM rch_mission_team_links ORDER BY mission_uid, team_uid",
            )?,
            self.load_payload_rows::<TeamMemberRecord>(
                "SELECT payload FROM rch_team_members ORDER BY uid",
            )?,
            self.load_payload_rows::<TeamMemberClientLinkRecord>(
                "SELECT payload FROM rch_team_member_client_links ORDER BY team_member_uid, client_identity",
            )?,
        ))
    }

    #[allow(clippy::too_many_lines)]
    pub fn load_r3akt_read_snapshot(&self) -> Result<RchCoreSnapshot, RchCoreError> {
        let topics = self
            .load_payload_rows::<TopicRecord>("SELECT payload FROM rch_topics ORDER BY topic_id")?;
        let subscribers = self.load_payload_rows::<SubscriberRecord>(
            "SELECT payload FROM rch_subscribers ORDER BY topic_id, node_id",
        )?;
        let clients = self.load_payload_rows::<ClientRecord>(
            "SELECT payload FROM rch_clients ORDER BY identity",
        )?;
        let identity_announces = self.load_payload_rows::<IdentityAnnounceRecord>(
            "SELECT payload FROM rch_identity_announces ORDER BY destination_hash",
        )?;
        let identity_states = self.load_payload_rows::<IdentityStateRecord>(
            "SELECT payload FROM rch_identity_states ORDER BY identity",
        )?;
        let identity_rem_modes = self.load_payload_rows::<IdentityRemModeRecord>(
            "SELECT payload FROM rch_identity_rem_modes ORDER BY identity",
        )?;
        let markers = self.load_payload_rows::<MarkerRecord>(
            "SELECT payload FROM rch_markers ORDER BY object_destination_hash",
        )?;
        let zones =
            self.load_payload_rows::<ZoneRecord>("SELECT payload FROM rch_zones ORDER BY zone_id")?;
        let missions = self
            .load_payload_rows::<MissionRecord>("SELECT payload FROM rch_missions ORDER BY uid")?;
        let mission_changes = self.load_payload_rows::<MissionChangeRecord>(
            "SELECT payload FROM rch_mission_changes ORDER BY uid",
        )?;
        let log_entries = self.load_payload_rows::<LogEntryRecord>(
            "SELECT payload FROM rch_log_entries ORDER BY entry_uid",
        )?;
        let eam_snapshots = self.load_payload_rows::<EamSnapshotRecord>(
            "SELECT payload FROM rch_eam_snapshots ORDER BY callsign",
        )?;
        let teams =
            self.load_payload_rows::<TeamRecord>("SELECT payload FROM rch_teams ORDER BY uid")?;
        let mission_team_links = self.load_payload_rows::<MissionTeamLinkRecord>(
            "SELECT payload FROM rch_mission_team_links ORDER BY mission_uid, team_uid",
        )?;
        let mission_zone_links = self.load_payload_rows::<MissionZoneLinkRecord>(
            "SELECT payload FROM rch_mission_zone_links ORDER BY mission_uid, zone_id",
        )?;
        let mission_marker_links = self.load_payload_rows::<MissionMarkerLinkRecord>(
            "SELECT payload FROM rch_mission_marker_links ORDER BY mission_uid, marker_id",
        )?;
        let team_members = self.load_payload_rows::<TeamMemberRecord>(
            "SELECT payload FROM rch_team_members ORDER BY uid",
        )?;
        let team_member_client_links = self.load_payload_rows::<TeamMemberClientLinkRecord>(
            "SELECT payload FROM rch_team_member_client_links ORDER BY team_member_uid, client_identity",
        )?;
        let assets = self.load_payload_rows::<AssetRecord>(
            "SELECT payload FROM rch_assets ORDER BY asset_uid",
        )?;
        let skills = self.load_payload_rows::<SkillRecord>(
            "SELECT payload FROM rch_skills ORDER BY skill_uid",
        )?;
        let team_member_skills = self.load_payload_rows::<TeamMemberSkillRecord>(
            "SELECT payload FROM rch_team_member_skills ORDER BY team_member_rns_identity, skill_uid",
        )?;
        let task_skill_requirements = self.load_payload_rows::<TaskSkillRequirementRecord>(
            "SELECT payload FROM rch_task_skill_requirements ORDER BY task_uid, skill_uid",
        )?;
        let assignments = self.load_payload_rows::<AssignmentRecord>(
            "SELECT payload FROM rch_assignments ORDER BY assignment_uid",
        )?;
        let assignment_asset_links = self.load_payload_rows::<AssignmentAssetLinkRecord>(
            "SELECT payload FROM rch_assignment_asset_links ORDER BY assignment_uid, asset_uid",
        )?;
        let (
            checklists,
            checklist_templates,
            checklist_columns,
            checklist_tasks,
            checklist_cells,
            checklist_feed_publications,
        ) = self.load_checklist_snapshot_rows()?;
        let identity_capabilities = self.load_payload_rows::<IdentityCapabilityGrant>(
            "SELECT payload FROM rch_identity_capabilities ORDER BY identity, capability",
        )?;
        let mission_access_assignments = self.load_payload_rows::<MissionAccessAssignment>(
            "SELECT payload FROM rch_mission_access_assignments ORDER BY mission_uid, subject_type, subject_id",
        )?;
        let subject_operation_rights = self.load_payload_rows::<SubjectOperationRight>(
            "SELECT payload FROM rch_subject_operation_rights
             ORDER BY subject_type, subject_id, operation, scope_type, scope_id",
        )?;
        let authorization_required = self
            .setting_value("authorization_required")?
            .is_some_and(|value| value == "true");
        Ok(RchCoreSnapshot {
            topics,
            subscribers,
            messages: Vec::new(),
            clients,
            identity_announces,
            identity_states,
            identity_rem_modes,
            audit_events: Vec::new(),
            system_events: Vec::new(),
            telemetry_records: Vec::new(),
            markers,
            zones,
            missions,
            mission_changes,
            log_entries,
            file_attachments: Vec::new(),
            eam_snapshots,
            teams,
            mission_team_links,
            mission_zone_links,
            mission_marker_links,
            team_members,
            team_member_client_links,
            assets,
            skills,
            team_member_skills,
            task_skill_requirements,
            assignments,
            assignment_asset_links,
            checklists,
            checklist_templates,
            checklist_columns,
            checklist_tasks,
            checklist_cells,
            checklist_feed_publications,
            command_results: Vec::new(),
            identity_capabilities,
            mission_access_assignments,
            subject_operation_rights,
            authorization_required,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn load_snapshot_rows(
        &self,
        include_identity_announces: bool,
    ) -> Result<RchCoreSnapshot, RchCoreError> {
        let topics = self
            .load_payload_rows::<TopicRecord>("SELECT payload FROM rch_topics ORDER BY topic_id")?;
        let subscribers = self.load_payload_rows::<SubscriberRecord>(
            "SELECT payload FROM rch_subscribers ORDER BY topic_id, node_id",
        )?;
        let messages = self
            .load_payload_rows::<MessageRecord>("SELECT payload FROM rch_messages ORDER BY id")?;
        let clients = self.load_payload_rows::<ClientRecord>(
            "SELECT payload FROM rch_clients ORDER BY identity",
        )?;
        let identity_announces = if include_identity_announces {
            self.load_payload_rows::<IdentityAnnounceRecord>(
                "SELECT payload FROM rch_identity_announces ORDER BY destination_hash",
            )?
        } else {
            Vec::new()
        };
        let identity_states = self.load_payload_rows::<IdentityStateRecord>(
            "SELECT payload FROM rch_identity_states ORDER BY identity",
        )?;
        let identity_rem_modes = self.load_payload_rows::<IdentityRemModeRecord>(
            "SELECT payload FROM rch_identity_rem_modes ORDER BY identity",
        )?;
        let audit_events = self.load_payload_rows::<MissionAuditEvent>(
            "SELECT payload FROM rch_audit_events ORDER BY id",
        )?;
        let system_events = self.load_payload_rows::<SystemEventRecord>(
            "SELECT payload FROM rch_system_events ORDER BY id",
        )?;
        let telemetry_records = self.load_latest_telemetry_records()?;
        let markers = self.load_payload_rows::<MarkerRecord>(
            "SELECT payload FROM rch_markers ORDER BY object_destination_hash",
        )?;
        let zones =
            self.load_payload_rows::<ZoneRecord>("SELECT payload FROM rch_zones ORDER BY zone_id")?;
        let missions = self
            .load_payload_rows::<MissionRecord>("SELECT payload FROM rch_missions ORDER BY uid")?;
        let mission_changes = self.load_payload_rows::<MissionChangeRecord>(
            "SELECT payload FROM rch_mission_changes ORDER BY uid",
        )?;
        let log_entries = self.load_payload_rows::<LogEntryRecord>(
            "SELECT payload FROM rch_log_entries ORDER BY entry_uid",
        )?;
        let file_attachments = self
            .load_payload_rows::<FileAttachmentRecord>(
                "SELECT payload FROM rch_file_attachments ORDER BY file_id",
            )
            .unwrap_or_default();
        let eam_snapshots = self.load_payload_rows::<EamSnapshotRecord>(
            "SELECT payload FROM rch_eam_snapshots ORDER BY callsign",
        )?;
        let teams =
            self.load_payload_rows::<TeamRecord>("SELECT payload FROM rch_teams ORDER BY uid")?;
        let mission_team_links = self.load_payload_rows::<MissionTeamLinkRecord>(
            "SELECT payload FROM rch_mission_team_links ORDER BY mission_uid, team_uid",
        )?;
        let mission_zone_links = self.load_payload_rows::<MissionZoneLinkRecord>(
            "SELECT payload FROM rch_mission_zone_links ORDER BY mission_uid, zone_id",
        )?;
        let mission_marker_links = self.load_payload_rows::<MissionMarkerLinkRecord>(
            "SELECT payload FROM rch_mission_marker_links ORDER BY mission_uid, marker_id",
        )?;
        let team_members = self.load_payload_rows::<TeamMemberRecord>(
            "SELECT payload FROM rch_team_members ORDER BY uid",
        )?;
        let team_member_client_links = self.load_payload_rows::<TeamMemberClientLinkRecord>(
            "SELECT payload FROM rch_team_member_client_links ORDER BY team_member_uid, client_identity",
        )?;
        let assets = self.load_payload_rows::<AssetRecord>(
            "SELECT payload FROM rch_assets ORDER BY asset_uid",
        )?;
        let skills = self.load_payload_rows::<SkillRecord>(
            "SELECT payload FROM rch_skills ORDER BY skill_uid",
        )?;
        let team_member_skills = self.load_payload_rows::<TeamMemberSkillRecord>(
            "SELECT payload FROM rch_team_member_skills ORDER BY team_member_rns_identity, skill_uid",
        )?;
        let task_skill_requirements = self.load_payload_rows::<TaskSkillRequirementRecord>(
            "SELECT payload FROM rch_task_skill_requirements ORDER BY task_uid, skill_uid",
        )?;
        let assignments = self.load_payload_rows::<AssignmentRecord>(
            "SELECT payload FROM rch_assignments ORDER BY assignment_uid",
        )?;
        let assignment_asset_links = self.load_payload_rows::<AssignmentAssetLinkRecord>(
            "SELECT payload FROM rch_assignment_asset_links ORDER BY assignment_uid, asset_uid",
        )?;
        let (
            checklists,
            checklist_templates,
            checklist_columns,
            checklist_tasks,
            checklist_cells,
            checklist_feed_publications,
        ) = self.load_checklist_snapshot_rows()?;
        let command_results = self.load_payload_rows::<CommandResultEnvelope>(
            "SELECT payload FROM rch_command_results ORDER BY command_id",
        )?;
        let identity_capabilities = self.load_payload_rows::<IdentityCapabilityGrant>(
            "SELECT payload FROM rch_identity_capabilities ORDER BY identity, capability",
        )?;
        let mission_access_assignments = self.load_payload_rows::<MissionAccessAssignment>(
            "SELECT payload FROM rch_mission_access_assignments ORDER BY mission_uid, subject_type, subject_id",
        )?;
        let subject_operation_rights = self.load_payload_rows::<SubjectOperationRight>(
            "SELECT payload FROM rch_subject_operation_rights
             ORDER BY subject_type, subject_id, operation, scope_type, scope_id",
        )?;
        let authorization_required = self
            .setting_value("authorization_required")?
            .is_some_and(|value| value == "true");
        Ok(RchCoreSnapshot {
            topics,
            subscribers,
            messages,
            clients,
            identity_announces,
            identity_states,
            identity_rem_modes,
            audit_events,
            system_events,
            telemetry_records,
            markers,
            zones,
            missions,
            mission_changes,
            log_entries,
            file_attachments,
            eam_snapshots,
            teams,
            mission_team_links,
            mission_zone_links,
            mission_marker_links,
            team_members,
            team_member_client_links,
            assets,
            skills,
            team_member_skills,
            task_skill_requirements,
            assignments,
            assignment_asset_links,
            checklists,
            checklist_templates,
            checklist_columns,
            checklist_tasks,
            checklist_cells,
            checklist_feed_publications,
            command_results,
            identity_capabilities,
            mission_access_assignments,
            subject_operation_rights,
            authorization_required,
        })
    }

    fn load_checklist_snapshot_rows(&self) -> Result<ChecklistSnapshotRows, RchCoreError> {
        Ok((
            self.load_payload_rows::<ChecklistRecord>(
                "SELECT payload FROM rch_checklists ORDER BY uid",
            )?,
            self.load_payload_rows::<ChecklistTemplateRecord>(
                "SELECT payload FROM rch_checklist_templates ORDER BY uid",
            )?,
            self.load_payload_rows::<ChecklistColumnRecord>(
                "SELECT payload FROM rch_checklist_columns ORDER BY checklist_uid, column_uid",
            )?,
            self.load_payload_rows::<ChecklistTaskRecord>(
                "SELECT payload FROM rch_checklist_tasks ORDER BY checklist_uid, task_uid",
            )?,
            self.load_payload_rows::<ChecklistCellRecord>(
                "SELECT payload FROM rch_checklist_cells ORDER BY task_uid, column_uid",
            )?,
            self.load_payload_rows::<ChecklistFeedPublicationRecord>(
                "SELECT payload FROM rch_checklist_feed_publications ORDER BY checklist_uid, published_ts_ms",
            )?,
        ))
    }

    pub fn load_checklist_list_value(&self) -> Result<Value, RchCoreError> {
        let (
            checklists,
            checklist_templates,
            checklist_columns,
            checklist_tasks,
            checklist_cells,
            checklist_feed_publications,
        ) = self.load_checklist_snapshot_rows()?;
        let mut snapshot = RchCore::new().snapshot();
        snapshot.checklists = checklists;
        snapshot.checklist_templates = checklist_templates;
        snapshot.checklist_columns = checklist_columns;
        snapshot.checklist_tasks = checklist_tasks;
        snapshot.checklist_cells = checklist_cells;
        snapshot.checklist_feed_publications = checklist_feed_publications;
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "checklists": core.checklist_values() }))
    }

    pub fn load_checklist_template_list_value(&self) -> Result<Value, RchCoreError> {
        let mut snapshot = RchCore::new().snapshot();
        snapshot.checklist_templates = self.load_payload_rows::<ChecklistTemplateRecord>(
            "SELECT payload FROM rch_checklist_templates ORDER BY uid",
        )?;
        snapshot.checklist_columns = self.load_payload_rows::<ChecklistColumnRecord>(
            "SELECT payload FROM rch_checklist_columns ORDER BY checklist_uid, column_uid",
        )?;
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "templates": core.checklist_template_values() }))
    }

    pub fn load_mission_list_value(&self, args: &Value) -> Result<Value, RchCoreError> {
        let snapshot = if mission_expand_values(args).is_empty() {
            let mut snapshot = RchCore::new().snapshot();
            snapshot.missions = self.load_payload_rows::<MissionRecord>(
                "SELECT payload FROM rch_missions ORDER BY uid",
            )?;
            snapshot.mission_zone_links = self.load_payload_rows::<MissionZoneLinkRecord>(
                "SELECT payload FROM rch_mission_zone_links ORDER BY mission_uid, zone_id",
            )?;
            snapshot.mission_marker_links = self.load_payload_rows::<MissionMarkerLinkRecord>(
                "SELECT payload FROM rch_mission_marker_links ORDER BY mission_uid, marker_id",
            )?;
            snapshot
        } else {
            self.load_r3akt_read_snapshot()?
        };
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "missions": core.limited_mission_values(args) }))
    }

    pub fn load_mission_change_list_value(
        &self,
        mission_uid: Option<&str>,
    ) -> Result<Value, RchCoreError> {
        let mut snapshot = RchCore::new().snapshot();
        snapshot.mission_changes = self.load_payload_rows::<MissionChangeRecord>(
            "SELECT payload FROM rch_mission_changes ORDER BY uid",
        )?;
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "mission_changes": core.mission_change_values(mission_uid) }))
    }

    pub fn load_log_entry_list_value(
        &self,
        mission_uid: Option<&str>,
        marker_ref: Option<&str>,
    ) -> Result<Value, RchCoreError> {
        let mut snapshot = RchCore::new().snapshot();
        snapshot.log_entries = self.load_payload_rows::<LogEntryRecord>(
            "SELECT payload FROM rch_log_entries ORDER BY entry_uid",
        )?;
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "log_entries": core.log_entry_values(mission_uid, marker_ref) }))
    }

    pub fn load_team_list_value(&self, mission_uid: Option<&str>) -> Result<Value, RchCoreError> {
        let mut snapshot = RchCore::new().snapshot();
        snapshot.teams =
            self.load_payload_rows::<TeamRecord>("SELECT payload FROM rch_teams ORDER BY uid")?;
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "teams": core.team_values(mission_uid) }))
    }

    pub fn load_team_member_list_value(
        &self,
        team_uid: Option<&str>,
    ) -> Result<Value, RchCoreError> {
        let mut snapshot = RchCore::new().snapshot();
        snapshot.team_members = self.load_payload_rows::<TeamMemberRecord>(
            "SELECT payload FROM rch_team_members ORDER BY uid",
        )?;
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "team_members": core.team_member_values(team_uid) }))
    }

    pub fn load_asset_list_value(
        &self,
        team_member_uid: Option<&str>,
    ) -> Result<Value, RchCoreError> {
        let mut snapshot = RchCore::new().snapshot();
        snapshot.assets = self.load_payload_rows::<AssetRecord>(
            "SELECT payload FROM rch_assets ORDER BY asset_uid",
        )?;
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "assets": core.asset_values(team_member_uid) }))
    }

    pub fn load_assignment_list_value(
        &self,
        mission_uid: Option<&str>,
        task_uid: Option<&str>,
    ) -> Result<Value, RchCoreError> {
        let mut snapshot = RchCore::new().snapshot();
        snapshot.assignments = self.load_payload_rows::<AssignmentRecord>(
            "SELECT payload FROM rch_assignments ORDER BY assignment_uid",
        )?;
        snapshot.assignment_asset_links = self.load_payload_rows::<AssignmentAssetLinkRecord>(
            "SELECT payload FROM rch_assignment_asset_links ORDER BY assignment_uid, asset_uid",
        )?;
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "assignments": core.assignment_values(mission_uid, task_uid) }))
    }

    pub fn load_eam_list_value(
        &self,
        team_uid: Option<&str>,
        overall_status: Option<&str>,
    ) -> Result<Value, RchCoreError> {
        let mut snapshot = RchCore::new().snapshot();
        snapshot.eam_snapshots = self.load_payload_rows::<EamSnapshotRecord>(
            "SELECT payload FROM rch_eam_snapshots ORDER BY callsign",
        )?;
        let core = RchCore::from_snapshot(snapshot)?;
        Ok(json!({ "eams": core.eam_values(team_uid, overall_status)? }))
    }

    pub fn load_audit_events_desc(
        &self,
        limit: usize,
    ) -> Result<Vec<MissionAuditEvent>, RchCoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM rch_audit_events ORDER BY id DESC LIMIT ?1")?;
        let rows = statement.query_map(params![limit], |row| row.get::<_, Vec<u8>>(0))?;
        let mut records: Vec<MissionAuditEvent> = Vec::new();
        for row in rows {
            records.push(decode_msgpack(&row?)?);
        }
        records.sort_by_key(|record| std::cmp::Reverse(record.timestamp_ms));
        Ok(records)
    }

    pub fn row_count(&self, table: &str) -> Result<usize, RchCoreError> {
        if !RCH_SQLITE_DATA_TABLES.contains(&table) {
            return Err(RchCoreError::InvalidPayload(format!(
                "unsupported RCH state table '{table}'"
            )));
        }
        let count =
            self.connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })?;
        usize::try_from(count).map_err(|error| RchCoreError::Decode(error.to_string()))
    }

    pub fn database_wipe_preview(&self) -> Result<RchDatabaseWipeReport, RchCoreError> {
        let mut tables = Vec::with_capacity(RCH_SQLITE_DATA_TABLES.len());
        let mut rows_deleted = 0usize;
        for table in RCH_SQLITE_DATA_TABLES {
            let count = self.row_count(table)?;
            rows_deleted = rows_deleted.saturating_add(count);
            tables.push(RchDatabaseWipeTableReport {
                table: (*table).to_string(),
                rows_deleted: count,
            });
        }
        Ok(RchDatabaseWipeReport {
            tables,
            rows_deleted,
        })
    }

    pub fn wipe_database(&mut self) -> Result<RchDatabaseWipeReport, RchCoreError> {
        let transaction = self.connection.transaction()?;
        let mut tables = Vec::with_capacity(RCH_SQLITE_DATA_TABLES.len());
        let mut rows_deleted = 0usize;
        for table in RCH_SQLITE_DATA_TABLES {
            let deleted = transaction.execute(&format!("DELETE FROM {table}"), [])?;
            rows_deleted = rows_deleted.saturating_add(deleted);
            tables.push(RchDatabaseWipeTableReport {
                table: (*table).to_string(),
                rows_deleted: deleted,
            });
        }
        if sqlite_table_exists(&transaction, "sqlite_sequence")? {
            transaction.execute("DELETE FROM sqlite_sequence WHERE name LIKE 'rch_%'", [])?;
        }
        transaction.commit()?;
        self.migrate()?;
        self.connection.execute_batch("VACUUM; PRAGMA optimize;")?;
        Ok(RchDatabaseWipeReport {
            tables,
            rows_deleted,
        })
    }

    fn migrate(&self) -> Result<(), RchCoreError> {
        let has_existing_schema = self.connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
            )",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        let has_migration_history = self.connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'rch_schema_migrations'
            )",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        let migration_2_applied = has_migration_history
            && self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM rch_schema_migrations WHERE version = 2)",
                [],
                |row| row.get::<_, bool>(0),
            )?;
        let migration_3_applied = has_migration_history
            && self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM rch_schema_migrations WHERE version = 3)",
                [],
                |row| row.get::<_, bool>(0),
            )?;
        if migration_3_applied {
            return Ok(());
        }
        if has_existing_schema {
            self.integrity_check()?;
            self.backup_before_migration(if migration_2_applied { 3 } else { 2 })?;
        }
        self.connection.execute_batch("BEGIN IMMEDIATE;")?;
        let migration_result = (|| -> Result<(), RchCoreError> {
            self.connection.execute_batch(RCH_SQLITE_MIGRATION_SQL)?;
            self.connection.execute_batch(
                "CREATE TABLE IF NOT EXISTS rch_schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_ts_ms INTEGER NOT NULL
                 );",
            )?;
            self.connection.execute(
                "INSERT OR IGNORE INTO rch_schema_migrations (version, name, applied_ts_ms)
                 VALUES (1, 'rch_core_snapshot', ?1)",
                [utc_now_ms()],
            )?;
            if !migration_2_applied {
                if !sqlite_table_has_column(
                    &self.connection,
                    "rch_checklist_feed_publications",
                    "published_ts_ms",
                )? {
                    self.connection.execute(
                        "ALTER TABLE rch_checklist_feed_publications
                 ADD COLUMN published_ts_ms INTEGER NOT NULL DEFAULT 0",
                        [],
                    )?;
                }
                if !sqlite_table_has_column(
                    &self.connection,
                    "rch_checklist_columns",
                    "display_order",
                )? {
                    self.connection.execute(
                        "ALTER TABLE rch_checklist_columns
                 ADD COLUMN display_order INTEGER NOT NULL DEFAULT 0",
                        [],
                    )?;
                }
                for (column, definition) in [
                    ("delivery_state", "TEXT NOT NULL DEFAULT 'queued'"),
                    ("dispatch_status", "TEXT NOT NULL DEFAULT 'queued'"),
                    ("next_attempt_at_ts_ms", "INTEGER"),
                    ("attempts", "INTEGER NOT NULL DEFAULT 0"),
                    ("priority", "INTEGER NOT NULL DEFAULT 0"),
                    ("batch_id", "TEXT"),
                    ("created_ts_ms", "INTEGER NOT NULL DEFAULT 0"),
                ] {
                    if !sqlite_table_has_column(&self.connection, "rch_messages", column)? {
                        self.connection.execute(
                            &format!("ALTER TABLE rch_messages ADD COLUMN {column} {definition}"),
                            [],
                        )?;
                    }
                }
                if !sqlite_table_has_column(&self.connection, "rch_mission_changes", "mission_uid")?
                {
                    self.connection.execute(
                "ALTER TABLE rch_mission_changes ADD COLUMN mission_uid TEXT NOT NULL DEFAULT ''",
                [],
            )?;
                }
                for (column, definition) in [
                    ("mission_uid", "TEXT NOT NULL DEFAULT ''"),
                    ("task_uid", "TEXT NOT NULL DEFAULT ''"),
                    ("team_member_rns_identity", "TEXT NOT NULL DEFAULT ''"),
                ] {
                    if !sqlite_table_has_column(&self.connection, "rch_assignments", column)? {
                        self.connection.execute(
                            &format!(
                                "ALTER TABLE rch_assignments ADD COLUMN {column} {definition}"
                            ),
                            [],
                        )?;
                    }
                }
                self.connection.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_rch_messages_queue_due
                ON rch_messages (delivery_state, dispatch_status, next_attempt_at_ts_ms, priority, id);
             CREATE INDEX IF NOT EXISTS idx_rch_messages_batch
                ON rch_messages (batch_id);
             CREATE INDEX IF NOT EXISTS idx_rch_messages_created
                ON rch_messages (created_ts_ms);
             CREATE INDEX IF NOT EXISTS idx_rch_messages_queue_due_partial
                ON rch_messages (next_attempt_at_ts_ms, priority, id)
                WHERE delivery_state = 'queued';
             CREATE INDEX IF NOT EXISTS idx_rch_checklist_tasks_checklist
                ON rch_checklist_tasks (checklist_uid, task_uid);
             CREATE INDEX IF NOT EXISTS idx_rch_checklist_cells_task_column
                ON rch_checklist_cells (task_uid, column_uid);
             CREATE INDEX IF NOT EXISTS idx_rch_checklist_columns_checklist
                ON rch_checklist_columns (checklist_uid, display_order, column_uid);
             CREATE INDEX IF NOT EXISTS idx_rch_checklist_columns_template
                ON rch_checklist_columns (template_uid, display_order, column_uid);
             CREATE INDEX IF NOT EXISTS idx_rch_mission_changes_mission
                ON rch_mission_changes (mission_uid, uid);
             CREATE INDEX IF NOT EXISTS idx_rch_assignments_mission_task
                ON rch_assignments (mission_uid, task_uid, team_member_rns_identity, assignment_uid);
             CREATE INDEX IF NOT EXISTS idx_rch_telemetry_timestamp_peer
                ON rch_telemetry_records (timestamp_s, peer_destination);
             CREATE INDEX IF NOT EXISTS idx_rch_file_attachments_category
                ON rch_file_attachments (category, file_id);
             CREATE INDEX IF NOT EXISTS idx_rch_eam_active_team_member
                ON rch_eam_snapshots (team_uid, team_member_uid, deleted_ts_ms);
             CREATE INDEX IF NOT EXISTS idx_rch_domain_events_sequence
                ON rch_domain_events (sequence);
             CREATE INDEX IF NOT EXISTS idx_rch_domain_events_collection
                ON rch_domain_events (collection, entity_id, sequence);
             CREATE INDEX IF NOT EXISTS idx_rch_outbound_jobs_due
                ON rch_outbound_jobs (delivery_state, dispatch_status, next_attempt_at_ts_ms, priority, created_ts_ms);
             CREATE INDEX IF NOT EXISTS idx_rch_outbound_jobs_due_partial
                ON rch_outbound_jobs (next_attempt_at_ts_ms, priority, created_ts_ms)
                WHERE delivery_state = 'queued';
             CREATE INDEX IF NOT EXISTS idx_rch_outbound_jobs_batch
                ON rch_outbound_jobs (batch_id);",
                )?;
                self.connection.execute_batch(RCH_SQLITE_MIGRATION_2_SQL)?;
                self.connection.execute(
                    "INSERT INTO rch_schema_migrations (version, name, applied_ts_ms)
                 VALUES (2, 'ordered_migrations', ?1)",
                    [utc_now_ms()],
                )?;
            }
            if !migration_3_applied {
                self.connection.execute_batch(RCH_SQLITE_MIGRATION_3_SQL)?;
                self.connection.execute(
                    "INSERT INTO rch_schema_migrations (version, name, applied_ts_ms)
                 VALUES (3, 'topic_subscription_corrections', ?1)",
                    [utc_now_ms()],
                )?;
            }
            self.connection.execute(
                "INSERT OR REPLACE INTO rch_settings (setting_key, setting_value)
             VALUES ('schema_version', ?1)",
                [RCH_SQLITE_SCHEMA_VERSION],
            )?;
            Ok(())
        })();
        match migration_result {
            Ok(()) => self.connection.execute_batch("COMMIT;")?,
            Err(error) => {
                if let Err(rollback_error) = self.connection.execute_batch("ROLLBACK;") {
                    eprintln!("SQLite migration rollback failed: {rollback_error}");
                }
                return Err(error);
            }
        }
        self.integrity_check()?;
        Ok(())
    }

    fn backup_before_migration(&self, version: u32) -> Result<(), RchCoreError> {
        let path: String = self.connection.query_row(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            [],
            |row| row.get(0),
        )?;
        if path.trim().is_empty() {
            return Ok(());
        }
        self.backup_to(format!("{path}.pre-migration-v{version}.bak"))
    }

    fn load_payload_rows<T>(&self, sql: &str) -> Result<Vec<T>, RchCoreError>
    where
        T: DeserializeOwned,
    {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        let mut records = Vec::new();
        for row in rows {
            records.push(decode_msgpack(&row?)?);
        }
        Ok(records)
    }

    fn load_payload_page<T>(
        &self,
        sql: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<T>, RchCoreError>
    where
        T: DeserializeOwned,
    {
        let limit = i64::try_from(limit.clamp(1, 10_000))
            .map_err(|error| RchCoreError::Decode(error.to_string()))?;
        let offset =
            i64::try_from(offset).map_err(|error| RchCoreError::Decode(error.to_string()))?;
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params![limit, offset], |row| row.get::<_, Vec<u8>>(0))?;
        let mut records = Vec::new();
        for row in rows {
            records.push(decode_msgpack(&row?)?);
        }
        Ok(records)
    }

    fn load_latest_telemetry_records(&self) -> Result<Vec<TelemetryRecord>, RchCoreError> {
        self.load_payload_rows::<TelemetryRecord>(
            "SELECT payload
             FROM rch_telemetry_records AS keep
             WHERE keep.id = (
                 SELECT candidate.id
                 FROM rch_telemetry_records AS candidate
                 WHERE lower(candidate.peer_destination) = lower(keep.peer_destination)
                 ORDER BY candidate.timestamp_s DESC, candidate.id DESC
                 LIMIT 1
             )
             ORDER BY lower(keep.peer_destination), keep.timestamp_s",
        )
    }

    pub fn setting_value(&self, key: &str) -> Result<Option<String>, RchCoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT setting_value FROM rch_settings WHERE setting_key = ?1")?;
        let mut rows = statement.query([key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(row.get(0)?))
    }

    pub fn set_setting_value(&self, key: &str, value: &str) -> Result<(), RchCoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO rch_settings (setting_key, setting_value)
             VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    fn settings_with_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>, RchCoreError> {
        let pattern = format!("{prefix}%");
        let mut statement = self.connection.prepare(
            "SELECT setting_key, setting_value FROM rch_settings WHERE setting_key LIKE ?1",
        )?;
        let rows = statement.query_map([pattern], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut settings = Vec::new();
        for row in rows {
            settings.push(row?);
        }
        Ok(settings)
    }
}

fn configure_sqlite_connection(
    connection: &Connection,
    profile: RchSqliteConnectionProfile,
) -> Result<(), RchCoreError> {
    let busy_timeout_ms = match profile {
        RchSqliteConnectionProfile::ReadOnly => RCH_SQLITE_READ_BUSY_TIMEOUT_MS,
        RchSqliteConnectionProfile::Write => RCH_SQLITE_WRITE_BUSY_TIMEOUT_MS,
        RchSqliteConnectionProfile::Admin => RCH_SQLITE_ADMIN_BUSY_TIMEOUT_MS,
    };
    connection.busy_timeout(Duration::from_millis(busy_timeout_ms))?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;",
    )?;
    match profile {
        RchSqliteConnectionProfile::ReadOnly => {
            connection.execute_batch("PRAGMA query_only = ON;")?;
        }
        RchSqliteConnectionProfile::Write | RchSqliteConnectionProfile::Admin => {
            connection.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;",
            )?;
        }
    }
    Ok(())
}

fn sqlite_pragma_u64(connection: &Connection, name: &str) -> Result<u64, RchCoreError> {
    let value = connection.query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, i64>(0))?;
    u64::try_from(value).map_err(|error| RchCoreError::Decode(error.to_string()))
}

impl RchCore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn topics(&self) -> Vec<TopicRecord> {
        let mut records: Vec<_> = self.topics.values().cloned().collect();
        records.sort_by(|left, right| {
            left.created_ts_ms
                .cmp(&right.created_ts_ms)
                .then_with(|| left.topic_id.cmp(&right.topic_id))
        });
        records
    }

    #[must_use]
    pub fn subscribers(&self, topic_id: &str) -> Vec<SubscriberRecord> {
        let Some(topic_id) = normalize_topic_id(Some(topic_id)) else {
            return Vec::new();
        };
        let mut records: Vec<_> = self
            .subscribers
            .iter()
            .filter(|((_, subscribed_topic), _)| subscribed_topic == &topic_id)
            .map(|(_, record)| record.clone())
            .collect();
        records.sort_by(|left, right| left.node_id.cmp(&right.node_id));
        records
    }

    #[must_use]
    pub fn messages(&self) -> &[MessageRecord] {
        &self.messages
    }

    #[must_use]
    pub fn clients(&self) -> Vec<ClientRecord> {
        let mut records: Vec<_> = self.clients.values().cloned().collect();
        records.sort_by(|left, right| left.identity.cmp(&right.identity));
        records
    }

    fn client_values(&self) -> Vec<Value> {
        self.clients()
            .into_iter()
            .map(|client| {
                json!({
                    "identity": client.identity,
                    "nickname": client.nickname,
                    "role": client.role,
                    "paused": client.paused,
                    "text_only": client.text_only,
                    "first_seen_ts_ms": client.first_seen_ts_ms,
                    "last_seen_ts_ms": client.last_seen_ts_ms,
                    "last_chat_ts_ms": client.last_chat_ts_ms,
                })
            })
            .collect()
    }

    #[must_use]
    pub fn identity_announces(&self) -> Vec<IdentityAnnounceRecord> {
        let mut records: Vec<_> = self.identity_announces.values().cloned().collect();
        records.sort_by(|left, right| left.destination_hash.cmp(&right.destination_hash));
        records
    }

    #[must_use]
    pub fn identity_states(&self) -> Vec<IdentityStateRecord> {
        let mut records: Vec<_> = self.identity_states.values().cloned().collect();
        records.sort_by(|left, right| left.identity.cmp(&right.identity));
        records
    }

    #[must_use]
    pub fn identity_rem_modes(&self) -> Vec<IdentityRemModeRecord> {
        let mut records: Vec<_> = self.identity_rem_modes.values().cloned().collect();
        records.sort_by(|left, right| left.identity.cmp(&right.identity));
        records
    }

    #[must_use]
    pub fn audit_events(&self) -> &[MissionAuditEvent] {
        &self.audit_events
    }

    #[must_use]
    pub fn system_events(&self) -> &[SystemEventRecord] {
        &self.system_events
    }

    #[must_use]
    pub fn telemetry_records(&self) -> &[TelemetryRecord] {
        &self.telemetry_records
    }

    #[must_use]
    pub fn markers(&self) -> Vec<MarkerRecord> {
        let mut records: Vec<_> = self.markers.values().cloned().collect();
        records.sort_by(|left, right| {
            left.object_destination_hash
                .cmp(&right.object_destination_hash)
        });
        records
    }

    #[must_use]
    pub fn zones(&self) -> Vec<ZoneRecord> {
        let mut records: Vec<_> = self.zones.values().cloned().collect();
        records.sort_by(|left, right| left.zone_id.cmp(&right.zone_id));
        records
    }

    #[must_use]
    pub fn missions(&self) -> Vec<MissionRecord> {
        let mut records: Vec<_> = self.missions.values().cloned().collect();
        records.sort_by(|left, right| {
            right
                .created_ts_ms
                .cmp(&left.created_ts_ms)
                .then(left.uid.cmp(&right.uid))
        });
        records
    }

    #[must_use]
    pub fn mission_changes(&self) -> Vec<MissionChangeRecord> {
        let mut records: Vec<_> = self.mission_changes.values().cloned().collect();
        records.sort_by(|left, right| {
            right
                .timestamp_ms
                .cmp(&left.timestamp_ms)
                .then(left.uid.cmp(&right.uid))
        });
        records
    }

    #[must_use]
    pub fn file_attachments(&self) -> Vec<FileAttachmentRecord> {
        let mut records: Vec<_> = self.file_attachments.values().cloned().collect();
        records.sort_by_key(|record| record.file_id);
        records
    }

    #[must_use]
    pub fn log_entries(&self) -> Vec<LogEntryRecord> {
        let mut records: Vec<_> = self.log_entries.values().cloned().collect();
        records.sort_by(|left, right| {
            right
                .server_time_ms
                .cmp(&left.server_time_ms)
                .then(left.entry_uid.cmp(&right.entry_uid))
        });
        records
    }

    #[must_use]
    pub fn eam_snapshots(&self) -> Vec<EamSnapshotRecord> {
        let mut records: Vec<_> = self.eam_snapshots.values().cloned().collect();
        records.sort_by(|left, right| {
            left.callsign
                .cmp(&right.callsign)
                .then(left.team_member_uid.cmp(&right.team_member_uid))
        });
        records
    }

    #[must_use]
    pub fn teams(&self) -> Vec<TeamRecord> {
        let mut records: Vec<_> = self.teams.values().cloned().collect();
        records.sort_by(|left, right| left.team_name.cmp(&right.team_name));
        records
    }

    #[must_use]
    pub fn team_members(&self) -> Vec<TeamMemberRecord> {
        let mut records: Vec<_> = self.team_members.values().cloned().collect();
        records.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        records
    }

    #[must_use]
    pub fn assets(&self) -> Vec<AssetRecord> {
        let mut records: Vec<_> = self.assets.values().cloned().collect();
        records.sort_by(|left, right| left.name.cmp(&right.name));
        records
    }

    #[must_use]
    pub fn skills(&self) -> Vec<SkillRecord> {
        let mut records: Vec<_> = self.skills.values().cloned().collect();
        records.sort_by(|left, right| left.name.cmp(&right.name));
        records
    }

    #[must_use]
    pub fn team_member_skills(&self) -> Vec<TeamMemberSkillRecord> {
        let mut records: Vec<_> = self.team_member_skills.values().cloned().collect();
        records.sort_by(|left, right| {
            left.team_member_rns_identity
                .cmp(&right.team_member_rns_identity)
                .then(left.skill_uid.cmp(&right.skill_uid))
        });
        records
    }

    #[must_use]
    pub fn task_skill_requirements(&self) -> Vec<TaskSkillRequirementRecord> {
        let mut records: Vec<_> = self.task_skill_requirements.values().cloned().collect();
        records.sort_by(|left, right| {
            left.task_uid
                .cmp(&right.task_uid)
                .then(left.skill_uid.cmp(&right.skill_uid))
        });
        records
    }

    #[must_use]
    pub fn assignments(&self) -> Vec<AssignmentRecord> {
        let mut records: Vec<_> = self.assignments.values().cloned().collect();
        records.sort_by(|left, right| {
            right
                .assigned_ts_ms
                .cmp(&left.assigned_ts_ms)
                .then(left.assignment_uid.cmp(&right.assignment_uid))
        });
        records
    }

    #[must_use]
    pub fn checklists(&self) -> Vec<ChecklistRecord> {
        let mut records: Vec<_> = self.checklists.values().cloned().collect();
        records.sort_by(|left, right| left.name.cmp(&right.name));
        records
    }

    #[must_use]
    pub fn checklist_templates(&self) -> Vec<ChecklistTemplateRecord> {
        let mut records: Vec<_> = self.checklist_templates.values().cloned().collect();
        records.sort_by(|left, right| left.template_name.cmp(&right.template_name));
        records
    }

    #[must_use]
    pub fn checklist_columns(&self) -> Vec<ChecklistColumnRecord> {
        let mut records: Vec<_> = self.checklist_columns.values().cloned().collect();
        records.sort_by(|left, right| {
            left.checklist_uid
                .cmp(&right.checklist_uid)
                .then(left.template_uid.cmp(&right.template_uid))
                .then(left.display_order.cmp(&right.display_order))
        });
        records
    }

    #[must_use]
    pub fn checklist_tasks(&self) -> Vec<ChecklistTaskRecord> {
        let mut records: Vec<_> = self.checklist_tasks.values().cloned().collect();
        records.sort_by(|left, right| {
            left.checklist_uid
                .cmp(&right.checklist_uid)
                .then(left.number.cmp(&right.number))
        });
        records
    }

    #[must_use]
    pub fn checklist_cells(&self) -> Vec<ChecklistCellRecord> {
        let mut records: Vec<_> = self.checklist_cells.values().cloned().collect();
        records.sort_by(|left, right| {
            left.task_uid
                .cmp(&right.task_uid)
                .then(left.column_uid.cmp(&right.column_uid))
        });
        records
    }

    #[must_use]
    pub fn checklist_feed_publications(&self) -> Vec<ChecklistFeedPublicationRecord> {
        let mut records: Vec<_> = self.checklist_feed_publications.values().cloned().collect();
        records.sort_by(|left, right| {
            left.checklist_uid
                .cmp(&right.checklist_uid)
                .then(right.published_ts_ms.cmp(&left.published_ts_ms))
                .then(left.publication_uid.cmp(&right.publication_uid))
        });
        records
    }

    pub fn set_authorization_required(&mut self, required: bool) {
        self.authorization_required = required;
    }

    pub fn grant_identity_capability(
        &mut self,
        identity: impl AsRef<str>,
        capability: impl Into<String>,
    ) {
        if let Some(identity) = normalize_hash(Some(identity.as_ref())) {
            self.identity_capabilities
                .entry(identity)
                .or_default()
                .insert(capability.into());
        }
    }

    pub fn revoke_identity_capability(
        &mut self,
        identity: impl AsRef<str>,
        capability: impl AsRef<str>,
    ) -> bool {
        let Some(identity) = normalize_hash(Some(identity.as_ref())) else {
            return false;
        };
        let Some(capabilities) = self.identity_capabilities.get_mut(&identity) else {
            return false;
        };
        let revoked = capabilities.remove(capability.as_ref());
        if capabilities.is_empty() {
            self.identity_capabilities.remove(&identity);
        }
        revoked
    }

    pub fn grant_operation_right(
        &mut self,
        subject_type: impl AsRef<str>,
        subject_id: impl AsRef<str>,
        operation: impl AsRef<str>,
        scope_type: impl AsRef<str>,
        scope_id: impl AsRef<str>,
    ) -> Result<SubjectOperationRight, RchCoreError> {
        self.upsert_operation_right(
            subject_type,
            subject_id,
            operation,
            scope_type,
            scope_id,
            true,
        )
    }

    pub fn revoke_operation_right(
        &mut self,
        subject_type: impl AsRef<str>,
        subject_id: impl AsRef<str>,
        operation: impl AsRef<str>,
        scope_type: impl AsRef<str>,
        scope_id: impl AsRef<str>,
    ) -> Result<SubjectOperationRight, RchCoreError> {
        self.upsert_operation_right(
            subject_type,
            subject_id,
            operation,
            scope_type,
            scope_id,
            false,
        )
    }

    fn upsert_operation_right(
        &mut self,
        subject_type: impl AsRef<str>,
        subject_id: impl AsRef<str>,
        operation: impl AsRef<str>,
        scope_type: impl AsRef<str>,
        scope_id: impl AsRef<str>,
        granted: bool,
    ) -> Result<SubjectOperationRight, RchCoreError> {
        let subject_type = normalize_subject_type(subject_type.as_ref())?;
        let subject_id = normalize_subject_id(&subject_type, subject_id.as_ref())?;
        let operation = required_non_empty(operation.as_ref(), "operation")?;
        let scope_type = normalize_scope_type(scope_type.as_ref())?;
        let scope_id = normalize_scope_id(&scope_type, scope_id.as_ref());
        let key = (
            subject_type.clone(),
            subject_id.clone(),
            operation.clone(),
            scope_type.clone(),
            scope_id.clone(),
        );
        let grant_uid = self.subject_operation_rights.get(&key).map_or_else(
            || Uuid::new_v4().simple().to_string(),
            |record| record.grant_uid.clone(),
        );
        let record = SubjectOperationRight {
            grant_uid,
            subject_type,
            subject_id,
            operation,
            scope_type,
            scope_id,
            granted,
        };
        self.subject_operation_rights.insert(key, record.clone());
        Ok(record)
    }

    pub fn record_identity_announce(
        &mut self,
        identity: impl AsRef<str>,
        announced_identity_hash: Option<String>,
        display_name: Option<String>,
        source_interface: Option<String>,
        announce_capabilities: Vec<String>,
    ) -> Result<(), RchCoreError> {
        let destination_hash = normalize_hash(Some(identity.as_ref()))
            .ok_or_else(|| RchCoreError::InvalidPayload("identity is required".to_string()))?;
        let announced_identity_hash =
            announced_identity_hash.and_then(|value| normalize_hash(Some(&value)));
        let display_name = display_name.and_then(|value| {
            let value = value.trim().to_string();
            (!value.is_empty()).then_some(value)
        });
        let source_interface = source_interface.and_then(|value| {
            let value = value.trim().to_ascii_lowercase();
            (!value.is_empty()).then_some(value)
        });
        let announce_capabilities = normalize_announce_capabilities(announce_capabilities);
        let client_type = classify_client_type(&announce_capabilities);
        let now = utc_now_ms();
        self.identity_announces
            .entry(destination_hash.clone())
            .and_modify(|record| {
                record.last_seen_ts_ms = now;
                if let Some(value) = announced_identity_hash.clone() {
                    record.announced_identity_hash = Some(value);
                }
                if let Some(value) = display_name.clone() {
                    record.display_name = Some(value);
                }
                if let Some(value) = source_interface.clone() {
                    record.source_interface = Some(value);
                }
                if !announce_capabilities.is_empty() {
                    record
                        .announce_capabilities
                        .clone_from(&announce_capabilities);
                    record.client_type.clone_from(&client_type);
                }
            })
            .or_insert_with(|| IdentityAnnounceRecord {
                destination_hash,
                announced_identity_hash,
                display_name,
                source_interface,
                announce_capabilities,
                client_type,
                first_seen_ts_ms: now,
                last_seen_ts_ms: now,
            });
        Ok(())
    }

    pub fn upsert_team_member_from_identity_announce(
        &mut self,
        announce: &IdentityAnnounceRecord,
    ) -> Result<TeamMemberRecord, RchCoreError> {
        let identity = announce
            .announced_identity_hash
            .as_deref()
            .and_then(|value| normalize_hash(Some(value)))
            .or_else(|| normalize_hash(Some(&announce.destination_hash)))
            .ok_or_else(|| {
                RchCoreError::InvalidPayload("identity announce key is required".to_string())
            })?;
        let display_name = announce
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map_or_else(|| identity.clone(), str::to_string);
        let uid = self
            .team_members
            .iter()
            .find_map(|(uid, member)| {
                (normalize_hash(Some(&member.rns_identity)).as_deref() == Some(identity.as_str()))
                    .then(|| uid.clone())
            })
            .unwrap_or_else(|| identity.clone());
        let client_identities = self.team_member_client_ids(&uid);
        let now = utc_now_ms();
        let member = self
            .team_members
            .entry(uid.clone())
            .and_modify(|member| {
                member.rns_identity.clone_from(&identity);
                member.display_name.clone_from(&display_name);
                member.client_identities.clone_from(&client_identities);
                member.updated_ts_ms = now;
            })
            .or_insert_with(|| TeamMemberRecord {
                uid,
                team_uid: None,
                rns_identity: identity,
                display_name,
                icon: None,
                role: None,
                callsign: None,
                freq: None,
                email: None,
                phone: None,
                modulation: None,
                availability: None,
                certifications: Vec::new(),
                last_active: None,
                client_identities,
                created_ts_ms: now,
                updated_ts_ms: now,
            });
        Ok(member.clone())
    }

    pub fn set_identity_state(
        &mut self,
        identity: impl AsRef<str>,
        is_banned: bool,
        is_blackholed: bool,
    ) -> Result<IdentityStateRecord, RchCoreError> {
        let identity = normalize_hash(Some(identity.as_ref()))
            .ok_or_else(|| RchCoreError::InvalidPayload("identity is required".to_string()))?;
        let record = IdentityStateRecord {
            identity: identity.clone(),
            is_banned,
            is_blackholed,
            updated_ts_ms: utc_now_ms(),
        };
        self.identity_states.insert(identity, record.clone());
        Ok(record)
    }

    pub fn set_identity_rem_mode(
        &mut self,
        identity: impl AsRef<str>,
        mode: impl AsRef<str>,
    ) -> Result<IdentityRemModeRecord, RchCoreError> {
        let identity = normalize_hash(Some(identity.as_ref()))
            .ok_or_else(|| RchCoreError::InvalidPayload("identity is required".to_string()))?;
        let mode = normalize_rem_mode(mode.as_ref())?;
        let record = IdentityRemModeRecord {
            identity: identity.clone(),
            mode,
            updated_ts_ms: utc_now_ms(),
        };
        self.identity_rem_modes.insert(identity, record.clone());
        Ok(record)
    }

    #[must_use]
    pub fn has_identity_capability(&self, identity: &str, capability: &str) -> bool {
        normalize_hash(Some(identity))
            .and_then(|identity| self.identity_capabilities.get(&identity))
            .is_some_and(|capabilities| capabilities.contains(capability))
    }

    pub fn assign_mission_access_role(
        &mut self,
        mission_uid: impl AsRef<str>,
        subject_type: impl AsRef<str>,
        subject_id: impl AsRef<str>,
        role: impl AsRef<str>,
    ) -> Result<(), RchCoreError> {
        let mission_uid = required_non_empty(mission_uid.as_ref(), "mission_uid")?;
        let subject_type = normalize_subject_type(subject_type.as_ref())?;
        let subject_id = normalize_subject_id(&subject_type, subject_id.as_ref())?;
        let role = normalize_mission_role(role.as_ref(), "role")?;
        self.mission_access_assignments.insert(
            (
                mission_uid.clone(),
                subject_type.clone(),
                subject_id.clone(),
            ),
            MissionAccessAssignment {
                mission_uid,
                subject_type,
                subject_id,
                role,
            },
        );
        Ok(())
    }

    fn has_mission_access_operation(
        &self,
        command: &MissionCommandEnvelope,
        operation: &str,
    ) -> bool {
        self.candidate_mission_uids(command)
            .iter()
            .any(|mission_uid| {
                self.identity_has_mission_operation(
                    &command.source.rns_identity,
                    mission_uid,
                    operation,
                )
            })
    }

    fn identity_has_mission_operation(
        &self,
        identity: &str,
        mission_uid: &str,
        operation: &str,
    ) -> bool {
        self.authorize_identity_operation(identity, operation, Some(mission_uid))
    }

    #[must_use]
    pub fn authorize_identity_operation(
        &self,
        identity: &str,
        operation: &str,
        mission_uid: Option<&str>,
    ) -> bool {
        self.resolve_effective_operations(identity, mission_uid)
            .iter()
            .any(|candidate| candidate == operation)
    }

    #[must_use]
    pub fn resolve_effective_operations(
        &self,
        identity: &str,
        mission_uid: Option<&str>,
    ) -> Vec<String> {
        let subject_refs = self.subject_refs_for_identity(identity);
        let mission_uid = mission_uid.and_then(|value| {
            let value = value.trim();
            (!value.is_empty()).then_some(value)
        });
        let mut granted_operations = HashSet::new();
        let mut denied_operations = HashSet::new();
        if let Some(identity) = normalize_hash(Some(identity)) {
            if let Some(capabilities) = self.identity_capabilities.get(&identity) {
                granted_operations.extend(capabilities.iter().cloned());
            }
        }
        for record in self.matching_operation_rights(&subject_refs, mission_uid) {
            if record.granted {
                granted_operations.insert(record.operation.clone());
            } else {
                denied_operations.insert(record.operation.clone());
            }
        }
        for assignment in self.matching_mission_access_assignments(&subject_refs, mission_uid) {
            for operation in mission_role_operations(&assignment.role) {
                granted_operations.insert(operation.to_string());
            }
        }
        for operation in denied_operations {
            granted_operations.remove(&operation);
        }
        let mut operations: Vec<_> = granted_operations.into_iter().collect();
        operations.sort();
        operations
    }

    fn matching_operation_rights(
        &self,
        subject_refs: &[(String, String)],
        mission_uid: Option<&str>,
    ) -> Vec<&SubjectOperationRight> {
        self.subject_operation_rights
            .values()
            .filter(|record| {
                subject_refs.iter().any(|(subject_type, subject_id)| {
                    subject_type == &record.subject_type && subject_id == &record.subject_id
                }) && match record.scope_type.as_str() {
                    "global" => true,
                    "mission" => {
                        mission_uid.is_some_and(|mission_uid| mission_uid == record.scope_id)
                    }
                    _ => false,
                }
            })
            .collect()
    }

    fn matching_mission_access_assignments(
        &self,
        subject_refs: &[(String, String)],
        mission_uid: Option<&str>,
    ) -> Vec<&MissionAccessAssignment> {
        self.mission_access_assignments
            .values()
            .filter(|assignment| {
                mission_uid.is_none_or(|mission_uid| assignment.mission_uid == mission_uid)
                    && subject_refs.iter().any(|(subject_type, subject_id)| {
                        subject_type == &assignment.subject_type
                            && subject_id == &assignment.subject_id
                    })
            })
            .collect()
    }

    fn subject_refs_for_identity(&self, identity: &str) -> Vec<(String, String)> {
        let Some(identity) = normalize_hash(Some(identity)) else {
            return Vec::new();
        };
        let mut refs = vec![("identity".to_string(), identity.clone())];
        for member in self.team_members.values() {
            if normalize_hash(Some(&member.rns_identity)).as_deref() == Some(identity.as_str()) {
                refs.push(("team_member".to_string(), member.uid.clone()));
            }
        }
        for (team_member_uid, client_identity) in &self.team_member_client_links {
            if normalize_hash(Some(client_identity)).as_deref() == Some(identity.as_str()) {
                refs.push(("team_member".to_string(), team_member_uid.clone()));
            }
        }
        refs.sort();
        refs.dedup();
        refs
    }

    fn candidate_mission_uids(&self, command: &MissionCommandEnvelope) -> Vec<String> {
        let mut mission_uids = HashSet::new();
        if let Some(mission_uid) = optional_text(&command.args, &["mission_uid", "mission_id"]) {
            mission_uids.insert(mission_uid);
        } else if command.command_type == "mission.registry.log_entry.upsert" {
            mission_uids.insert(DEFAULT_LOG_MISSION_UID.to_string());
        }
        if let Some(topic_id) = optional_text(&command.args, &["topic_id", "id"]) {
            mission_uids.extend(self.mission_uids_for_topic(&topic_id));
        }
        if command.command_type.starts_with("mission.registry.team.")
            || command.command_type.starts_with("mission.registry.eam.")
        {
            if let Some(team_uid) = optional_text(&command.args, &["team_uid", "uid"]) {
                mission_uids.extend(self.team_mission_ids(&team_uid));
            }
        }
        if command
            .command_type
            .starts_with("mission.registry.team_member.")
            || command.command_type.starts_with("mission.registry.eam.")
        {
            if let Some(member_uid) = optional_text(&command.args, &["team_member_uid", "uid"]) {
                mission_uids.extend(self.mission_uids_for_team_member(&member_uid));
            }
        }
        let mut mission_uids: Vec<_> = mission_uids.into_iter().collect();
        mission_uids.sort();
        mission_uids
    }

    fn mission_uids_for_topic(&self, topic_id: &str) -> Vec<String> {
        let normalized = normalize_topic_id(Some(topic_id));
        self.missions
            .values()
            .filter(|mission| {
                normalized
                    .as_deref()
                    .is_some_and(|topic_id| mission.topic_id.as_deref() == Some(topic_id))
            })
            .map(|mission| mission.uid.clone())
            .collect()
    }

    fn mission_uids_for_team_member(&self, member_uid: &str) -> Vec<String> {
        self.team_members
            .get(member_uid)
            .and_then(|member| member.team_uid.as_deref())
            .map_or_else(Vec::new, |team_uid| self.team_mission_ids(team_uid))
    }

    fn mission_uids_for_asset_upsert(
        &self,
        asset: &AssetRecord,
        previous_team_member_uid: Option<String>,
    ) -> Vec<String> {
        let mut mission_uids = Vec::new();
        if let Some(team_member_uid) = asset.team_member_uid.as_deref() {
            mission_uids.extend(self.mission_uids_for_team_member(team_member_uid));
        }
        if let Some(team_member_uid) = previous_team_member_uid {
            mission_uids.extend(self.mission_uids_for_team_member(&team_member_uid));
        }
        dedupe_non_empty(mission_uids)
    }

    fn mission_uids_for_asset_delete(&self, asset_uid: &str, asset: &AssetRecord) -> Vec<String> {
        let mut mission_uids = Vec::new();
        if let Some(team_member_uid) = asset.team_member_uid.as_deref() {
            mission_uids.extend(self.mission_uids_for_team_member(team_member_uid));
        }
        for (assignment_uid, linked_asset_uid) in &self.assignment_asset_links {
            if linked_asset_uid == asset_uid {
                if let Some(assignment) = self.assignments.get(assignment_uid) {
                    mission_uids.push(assignment.mission_uid.clone());
                }
            }
        }
        for assignment in self.assignments.values() {
            if assignment.assets.iter().any(|item| item == asset_uid) {
                mission_uids.push(assignment.mission_uid.clone());
            }
        }
        dedupe_non_empty(mission_uids)
    }

    fn has_checklist_access_operation(
        &self,
        command: &MissionCommandEnvelope,
        operation: &str,
    ) -> bool {
        self.candidate_checklist_mission_uid(command)
            .is_some_and(|mission_uid| {
                self.identity_has_mission_operation(
                    &command.source.rns_identity,
                    &mission_uid,
                    operation,
                )
            })
    }

    fn candidate_checklist_mission_uid(&self, command: &MissionCommandEnvelope) -> Option<String> {
        if let Some(mission_uid) = optional_text(&command.args, &["mission_uid", "mission_id"]) {
            return Some(mission_uid);
        }
        optional_text(&command.args, &["checklist_uid"]).and_then(|checklist_uid| {
            self.checklists
                .get(&checklist_uid)
                .and_then(|checklist| checklist.mission_uid.clone())
        })
    }

    pub fn save_to_sqlite(&self, store: &mut RchSqliteStore) -> Result<(), RchCoreError> {
        store.save_snapshot(&self.snapshot())
    }

    pub fn save_to_sqlite_preserving_identity_announces(
        &self,
        store: &mut RchSqliteStore,
    ) -> Result<(), RchCoreError> {
        store.save_snapshot_preserving_identity_announces(&self.snapshot())
    }

    pub fn load_from_sqlite(store: &RchSqliteStore) -> Result<Option<Self>, RchCoreError> {
        store.load_snapshot()?.map(Self::from_snapshot).transpose()
    }

    pub fn load_from_sqlite_without_identity_announces(
        store: &RchSqliteStore,
    ) -> Result<Option<Self>, RchCoreError> {
        store
            .load_snapshot_without_identity_announces()?
            .map(Self::from_snapshot)
            .transpose()
    }

    #[must_use]
    pub fn snapshot(&self) -> RchCoreSnapshot {
        RchCoreSnapshot {
            topics: self.topics(),
            subscribers: {
                let mut records: Vec<_> = self.subscribers.values().cloned().collect();
                records.sort_by(|left, right| {
                    left.topic_id
                        .cmp(&right.topic_id)
                        .then(left.node_id.cmp(&right.node_id))
                });
                records
            },
            messages: self.messages.clone(),
            clients: self.clients(),
            identity_announces: self.identity_announces(),
            identity_states: self.identity_states(),
            identity_rem_modes: self.identity_rem_modes(),
            audit_events: self.audit_events.clone(),
            system_events: self.system_events.clone(),
            telemetry_records: self.telemetry_records.clone(),
            markers: self.markers(),
            zones: self.zones(),
            missions: self.missions(),
            mission_changes: self.mission_changes(),
            log_entries: self.log_entries(),
            file_attachments: self.file_attachments(),
            eam_snapshots: self.eam_snapshots(),
            teams: self.teams(),
            mission_team_links: self.mission_team_link_records(),
            mission_zone_links: self.mission_zone_link_records(),
            mission_marker_links: self.mission_marker_link_records(),
            team_members: self.team_members(),
            team_member_client_links: self.team_member_client_link_records(),
            assets: self.assets(),
            skills: self.skills(),
            team_member_skills: self.team_member_skills(),
            task_skill_requirements: self.task_skill_requirements(),
            assignments: self.assignments(),
            assignment_asset_links: self.assignment_asset_link_records(),
            checklists: self.checklists(),
            checklist_templates: self.checklist_templates(),
            checklist_columns: self.checklist_columns(),
            checklist_tasks: self.checklist_tasks(),
            checklist_cells: self.checklist_cells(),
            checklist_feed_publications: self.checklist_feed_publications(),
            command_results: {
                let mut records: Vec<_> = self.command_results.values().cloned().collect();
                records.sort_by(|left, right| left.command_id.cmp(&right.command_id));
                records
            },
            identity_capabilities: self.identity_capability_grant_records(),
            mission_access_assignments: self.mission_access_assignment_records(),
            subject_operation_rights: self.subject_operation_right_records(),
            authorization_required: self.authorization_required,
        }
    }

    fn mission_team_link_records(&self) -> Vec<MissionTeamLinkRecord> {
        let mut records: Vec<_> = self
            .mission_team_links
            .iter()
            .map(|(mission_uid, team_uid)| MissionTeamLinkRecord {
                mission_uid: mission_uid.clone(),
                team_uid: team_uid.clone(),
            })
            .collect();
        records.sort_by(|left, right| {
            left.mission_uid
                .cmp(&right.mission_uid)
                .then(left.team_uid.cmp(&right.team_uid))
        });
        records
    }

    fn mission_zone_link_records(&self) -> Vec<MissionZoneLinkRecord> {
        let mut records: Vec<_> = self
            .mission_zone_links
            .iter()
            .map(|(mission_uid, zone_id)| MissionZoneLinkRecord {
                mission_uid: mission_uid.clone(),
                zone_id: zone_id.clone(),
            })
            .collect();
        records.sort_by(|left, right| {
            left.mission_uid
                .cmp(&right.mission_uid)
                .then(left.zone_id.cmp(&right.zone_id))
        });
        records
    }

    fn mission_marker_link_records(&self) -> Vec<MissionMarkerLinkRecord> {
        let mut records: Vec<_> = self
            .mission_marker_links
            .iter()
            .map(|(mission_uid, marker_id)| MissionMarkerLinkRecord {
                mission_uid: mission_uid.clone(),
                marker_id: marker_id.clone(),
            })
            .collect();
        records.sort_by(|left, right| {
            left.mission_uid
                .cmp(&right.mission_uid)
                .then(left.marker_id.cmp(&right.marker_id))
        });
        records
    }

    fn team_member_client_link_records(&self) -> Vec<TeamMemberClientLinkRecord> {
        let mut records: Vec<_> = self
            .team_member_client_links
            .iter()
            .map(
                |(team_member_uid, client_identity)| TeamMemberClientLinkRecord {
                    team_member_uid: team_member_uid.clone(),
                    client_identity: client_identity.clone(),
                },
            )
            .collect();
        records.sort_by(|left, right| {
            left.team_member_uid
                .cmp(&right.team_member_uid)
                .then(left.client_identity.cmp(&right.client_identity))
        });
        records
    }

    fn assignment_asset_link_records(&self) -> Vec<AssignmentAssetLinkRecord> {
        let mut records: Vec<_> = self
            .assignment_asset_links
            .iter()
            .map(|(assignment_uid, asset_uid)| AssignmentAssetLinkRecord {
                assignment_uid: assignment_uid.clone(),
                asset_uid: asset_uid.clone(),
            })
            .collect();
        records.sort_by(|left, right| {
            left.assignment_uid
                .cmp(&right.assignment_uid)
                .then(left.asset_uid.cmp(&right.asset_uid))
        });
        records
    }

    fn identity_capability_grant_records(&self) -> Vec<IdentityCapabilityGrant> {
        let mut records: Vec<_> = self
            .identity_capabilities
            .iter()
            .flat_map(|(identity, capabilities)| {
                capabilities
                    .iter()
                    .map(|capability| IdentityCapabilityGrant {
                        identity: identity.clone(),
                        capability: capability.clone(),
                    })
            })
            .collect();
        records.sort_by(|left, right| {
            left.identity
                .cmp(&right.identity)
                .then(left.capability.cmp(&right.capability))
        });
        records
    }

    fn mission_access_assignment_records(&self) -> Vec<MissionAccessAssignment> {
        let mut records: Vec<_> = self.mission_access_assignments.values().cloned().collect();
        records.sort_by(|left, right| {
            left.mission_uid
                .cmp(&right.mission_uid)
                .then(left.subject_type.cmp(&right.subject_type))
                .then(left.subject_id.cmp(&right.subject_id))
        });
        records
    }

    fn subject_operation_right_records(&self) -> Vec<SubjectOperationRight> {
        let mut records: Vec<_> = self.subject_operation_rights.values().cloned().collect();
        records.sort_by(|left, right| {
            left.subject_type
                .cmp(&right.subject_type)
                .then(left.subject_id.cmp(&right.subject_id))
                .then(left.operation.cmp(&right.operation))
                .then(left.scope_type.cmp(&right.scope_type))
                .then(left.scope_id.cmp(&right.scope_id))
        });
        records
    }

    fn validate_subscriber_invariants(&self) -> Result<(), RchCoreError> {
        let mut normalized_pairs = HashSet::new();
        for ((node_id, topic_id), subscriber) in &self.subscribers {
            let normalized_node_id =
                normalize_subscriber_node_id(Some(node_id)).ok_or_else(|| {
                    RchCoreError::InvalidPayload(
                        "subscriber invariant violation: destination is empty".to_string(),
                    )
                })?;
            let normalized_topic_id = normalize_subscriber_topic_id(topic_id).ok_or_else(|| {
                RchCoreError::InvalidPayload(
                    "subscriber invariant violation: topic_id is invalid".to_string(),
                )
            })?;
            if normalized_node_id != *node_id
                || normalized_topic_id != *topic_id
                || subscriber.node_id != *node_id
                || subscriber.topic_id != *topic_id
            {
                return Err(RchCoreError::InvalidPayload(
                    "subscriber invariant violation: key and record are not normalized or do not match"
                        .to_string(),
                ));
            }
            // The compatibility /Subscriber routes historically persist
            // topic-less and orphaned rows. New core subscriptions still
            // require an existing topic in `subscribe`, but loading those
            // rows must remain lossless and must not make R3AKT reads fail.
            if !normalized_pairs.insert((normalized_node_id, normalized_topic_id)) {
                return Err(RchCoreError::InvalidPayload(
                    "subscriber invariant violation: duplicate normalized destination/topic pair"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    pub fn from_snapshot(snapshot: RchCoreSnapshot) -> Result<Self, RchCoreError> {
        let mut core = Self::new();
        for mut topic in snapshot.topics {
            let topic_id = normalize_topic_id(Some(&topic.topic_id)).ok_or_else(|| {
                RchCoreError::InvalidPayload(
                    "topic invariant violation: topic_id is empty".to_string(),
                )
            })?;
            topic.topic_id.clone_from(&topic_id);
            if core.topics.insert(topic_id.clone(), topic).is_some() {
                return Err(RchCoreError::InvalidPayload(format!(
                    "topic invariant violation: duplicate normalized topic '{topic_id}'"
                )));
            }
        }
        for mut subscriber in snapshot.subscribers {
            let node_id =
                normalize_subscriber_node_id(Some(&subscriber.node_id)).ok_or_else(|| {
                    RchCoreError::InvalidPayload(
                        "subscriber invariant violation: destination is empty".to_string(),
                    )
                })?;
            let topic_id =
                normalize_subscriber_topic_id(&subscriber.topic_id).ok_or_else(|| {
                    RchCoreError::InvalidPayload(
                        "subscriber invariant violation: topic_id is invalid".to_string(),
                    )
                })?;
            subscriber.node_id.clone_from(&node_id);
            subscriber.topic_id.clone_from(&topic_id);
            let key = (node_id, topic_id);
            if core.subscribers.insert(key, subscriber).is_some() {
                return Err(RchCoreError::InvalidPayload(
                    "subscriber invariant violation: duplicate normalized destination/topic pair"
                        .to_string(),
                ));
            }
        }
        core.messages = snapshot.messages;
        for client in snapshot.clients {
            if let Some(identity) = normalize_hash(Some(&client.identity)) {
                core.clients.insert(identity, client);
            }
        }
        for announce in snapshot.identity_announces {
            core.identity_announces
                .insert(announce.destination_hash.clone(), announce);
        }
        for state in snapshot.identity_states {
            core.identity_states.insert(state.identity.clone(), state);
        }
        for rem_mode in snapshot.identity_rem_modes {
            core.identity_rem_modes
                .insert(rem_mode.identity.clone(), rem_mode);
        }
        core.audit_events = snapshot.audit_events;
        core.system_events = snapshot.system_events;
        core.telemetry_records = snapshot.telemetry_records;
        for marker in snapshot.markers {
            core.markers
                .insert(marker.object_destination_hash.clone(), marker);
        }
        for zone in snapshot.zones {
            core.zones.insert(zone.zone_id.clone(), zone);
        }
        for mission in snapshot.missions {
            core.missions.insert(mission.uid.clone(), mission);
        }
        for change in snapshot.mission_changes {
            core.mission_changes.insert(change.uid.clone(), change);
        }
        for entry in snapshot.log_entries {
            core.log_entries.insert(entry.entry_uid.clone(), entry);
        }
        for attachment in snapshot.file_attachments {
            core.file_attachments.insert(attachment.file_id, attachment);
        }
        for eam in snapshot.eam_snapshots {
            core.eam_snapshots.insert(eam.eam_uid.clone(), eam);
        }
        for team in snapshot.teams {
            core.teams.insert(team.uid.clone(), team);
        }
        for link in snapshot.mission_team_links {
            core.mission_team_links
                .insert((link.mission_uid, link.team_uid));
        }
        for link in snapshot.mission_zone_links {
            core.mission_zone_links
                .insert((link.mission_uid, link.zone_id));
        }
        for link in snapshot.mission_marker_links {
            core.mission_marker_links
                .insert((link.mission_uid, link.marker_id));
        }
        for member in snapshot.team_members {
            core.team_members.insert(member.uid.clone(), member);
        }
        for link in snapshot.team_member_client_links {
            core.team_member_client_links
                .insert((link.team_member_uid, link.client_identity));
        }
        for asset in snapshot.assets {
            core.assets.insert(asset.asset_uid.clone(), asset);
        }
        for skill in snapshot.skills {
            core.skills.insert(skill.skill_uid.clone(), skill);
        }
        for member_skill in snapshot.team_member_skills {
            core.team_member_skills.insert(
                format!(
                    "{}:{}",
                    member_skill.team_member_rns_identity, member_skill.skill_uid
                ),
                member_skill,
            );
        }
        for requirement in snapshot.task_skill_requirements {
            core.task_skill_requirements.insert(
                format!("{}:{}", requirement.task_uid, requirement.skill_uid),
                requirement,
            );
        }
        for assignment in snapshot.assignments {
            core.assignments
                .insert(assignment.assignment_uid.clone(), assignment);
        }
        for link in snapshot.assignment_asset_links {
            core.assignment_asset_links
                .insert((link.assignment_uid, link.asset_uid));
        }
        for checklist in snapshot.checklists {
            core.checklists.insert(checklist.uid.clone(), checklist);
        }
        for template in snapshot.checklist_templates {
            core.checklist_templates
                .insert(template.uid.clone(), template);
        }
        for column in snapshot.checklist_columns {
            core.checklist_columns
                .insert(column.column_uid.clone(), column);
        }
        for task in snapshot.checklist_tasks {
            core.checklist_tasks.insert(task.task_uid.clone(), task);
        }
        for cell in snapshot.checklist_cells {
            core.checklist_cells.insert(cell.cell_uid.clone(), cell);
        }
        for publication in snapshot.checklist_feed_publications {
            core.checklist_feed_publications
                .insert(publication.publication_uid.clone(), publication);
        }
        for result in snapshot.command_results {
            core.command_results
                .insert(result.command_id.clone(), result);
        }
        for grant in snapshot.identity_capabilities {
            core.grant_identity_capability(grant.identity, grant.capability);
        }
        for assignment in snapshot.mission_access_assignments {
            core.mission_access_assignments.insert(
                (
                    assignment.mission_uid.clone(),
                    assignment.subject_type.clone(),
                    assignment.subject_id.clone(),
                ),
                assignment,
            );
        }
        for right in snapshot.subject_operation_rights {
            core.subject_operation_rights.insert(
                (
                    right.subject_type.clone(),
                    right.subject_id.clone(),
                    right.operation.clone(),
                    right.scope_type.clone(),
                    right.scope_id.clone(),
                ),
                right,
            );
        }
        core.authorization_required = snapshot.authorization_required;
        core.validate_subscriber_invariants()?;
        Ok(core)
    }

    pub fn handle_command(&mut self, command: &MissionCommandEnvelope) -> RchCommandOutcome {
        if let Some(cached) = self.command_results.get(&command.command_id) {
            return RchCommandOutcome {
                result: cached.clone(),
                event: None,
            };
        }
        let outcome = match self.apply_command(command) {
            Ok(event) => {
                if let Some(event) = &event {
                    self.record_audit_event(command, event);
                }
                RchCommandOutcome {
                    result: CommandResultEnvelope {
                        command_id: command.command_id.clone(),
                        status: CommandResultStatus::Accepted,
                        detail: None,
                        reason_code: None,
                        reason: None,
                        required_capabilities: Vec::new(),
                        accepted_at: None,
                        by_identity: None,
                        correlation_id: command.correlation_id.clone(),
                        result: event
                            .as_ref()
                            .map_or(Value::Null, |event| event.payload.clone()),
                    },
                    event,
                }
            }
            Err(error) => RchCommandOutcome {
                result: CommandResultEnvelope {
                    command_id: command.command_id.clone(),
                    status: CommandResultStatus::Rejected,
                    detail: Some(error.to_string()),
                    reason_code: Some(error.reason_code().to_string()),
                    reason: Some(error.to_string()),
                    required_capabilities: Vec::new(),
                    accepted_at: None,
                    by_identity: None,
                    correlation_id: command.correlation_id.clone(),
                    result: Value::Null,
                },
                event: None,
            },
        };
        self.command_results
            .insert(command.command_id.clone(), outcome.result.clone());
        outcome
    }

    pub fn handle_mission_sync_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Vec<MissionSyncResponse> {
        if command.command_type.starts_with("rem.registry.") {
            return self.handle_rem_registry_sync_command(command);
        }
        if !is_supported_mission_command(command.command_type.as_str()) {
            return vec![MissionSyncResponse::results(Self::rejected_result(
                command,
                "unknown_command",
                format!("Unsupported mission command '{}'", command.command_type),
            ))];
        }
        if let Some(rejection) = self.rem_team_scope_rejection(command) {
            return vec![rejection];
        }
        if let Some(required_capability) = required_capability(command.command_type.as_str()) {
            if self.authorization_required
                && !self.has_identity_capability(&command.source.rns_identity, required_capability)
                && !self.has_mission_access_operation(command, required_capability)
            {
                return vec![MissionSyncResponse::results(
                    Self::rejected_result_with_capabilities(
                        command,
                        "unauthorized",
                        format!("Capability '{required_capability}' is required"),
                        vec![required_capability.to_string()],
                    ),
                )];
            }
        }

        let mut responses = vec![MissionSyncResponse::results(Self::accepted_result(command))];
        match self.apply_command(command) {
            Ok(event) => {
                if let Some(event) = &event {
                    self.record_audit_event(command, event);
                }
                let result_payload = event
                    .as_ref()
                    .map_or(Value::Null, |event| event.payload.clone());
                responses.push(MissionSyncResponse::results_with_event(
                    Self::completed_result(command, result_payload),
                    event
                        .as_ref()
                        .map(|event| Self::event_envelope_value(command, event)),
                ));
            }
            Err(error) => {
                responses.push(MissionSyncResponse::results(Self::rejected_result(
                    command,
                    error.reason_code(),
                    error.to_string(),
                )));
            }
        }
        responses
    }

    fn handle_rem_registry_sync_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Vec<MissionSyncResponse> {
        if !is_supported_rem_registry_command(command.command_type.as_str()) {
            return vec![MissionSyncResponse::rem_results(Self::rejected_result(
                command,
                "unknown_command",
                format!("Unsupported REM command '{}'", command.command_type),
            ))];
        }
        if let Some(rejection) = self.rem_registry_team_scope_rejection(command) {
            return vec![rejection];
        }
        let Some(source_identity) = normalize_hash(Some(&command.source.rns_identity)) else {
            return vec![MissionSyncResponse::rem_results(Self::rejected_result(
                command,
                "unauthorized",
                "Source identity is required",
            ))];
        };
        if !self.identity_has_rem_announce_capabilities(source_identity.as_str()) {
            return vec![MissionSyncResponse::rem_results(Self::rejected_result(
                command,
                "unauthorized",
                "REM announce capabilities are required",
            ))];
        }
        if command.command_type == "rem.registry.team_peers.list"
            && self
                .shared_team_uids_for_rem_source(source_identity.as_str())
                .is_empty()
        {
            return vec![MissionSyncResponse::rem_results(Self::rejected_result(
                command,
                "unauthorized",
                "TEAM membership is required",
            ))];
        }

        let mut responses = vec![MissionSyncResponse::rem_results(Self::accepted_result(
            command,
        ))];
        match self.apply_rem_registry_command(command) {
            Ok(event) => {
                if let Some(event) = &event {
                    self.record_audit_event(command, event);
                }
                let result_payload = event
                    .as_ref()
                    .map_or(Value::Null, |event| event.payload.clone());
                responses.push(MissionSyncResponse::rem_results_with_event(
                    Self::completed_result(command, result_payload),
                    event.as_ref().map(Self::rem_event_envelope_value),
                ));
            }
            Err(error) => {
                responses.push(MissionSyncResponse::rem_results(Self::rejected_result(
                    command,
                    error.reason_code(),
                    error.to_string(),
                )));
            }
        }
        responses
    }

    pub fn handle_checklist_sync_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Vec<MissionSyncResponse> {
        if !is_supported_checklist_command(command.command_type.as_str()) {
            return vec![MissionSyncResponse::results(Self::rejected_result(
                command,
                "unknown_command",
                format!("Unsupported checklist command '{}'", command.command_type),
            ))];
        }
        if let Some(rejection) = self.rem_team_scope_rejection(command) {
            return vec![rejection];
        }
        if let Some(required_capability) =
            checklist_required_capability(command.command_type.as_str())
        {
            if self.authorization_required
                && !self.has_identity_capability(&command.source.rns_identity, required_capability)
                && !self.has_checklist_access_operation(command, required_capability)
            {
                return vec![MissionSyncResponse::results(
                    Self::rejected_result_with_capabilities(
                        command,
                        "unauthorized",
                        format!("Capability '{required_capability}' is required"),
                        vec![required_capability.to_string()],
                    ),
                )];
            }
        }
        match self.apply_checklist_command(command) {
            Ok(event) => {
                self.record_audit_event(command, &event);
                Vec::new()
            }
            Err(error) => vec![MissionSyncResponse::results(Self::rejected_result(
                command,
                error.reason_code(),
                error.to_string(),
            ))],
        }
    }

    fn accepted_result(command: &MissionCommandEnvelope) -> CommandResultEnvelope {
        CommandResultEnvelope {
            command_id: command.command_id.clone(),
            status: CommandResultStatus::Accepted,
            detail: None,
            reason_code: None,
            reason: None,
            required_capabilities: Vec::new(),
            accepted_at: Some(utc_now_rfc3339()),
            by_identity: None,
            correlation_id: command.correlation_id.clone(),
            result: Value::Null,
        }
    }

    fn completed_result(command: &MissionCommandEnvelope, result: Value) -> CommandResultEnvelope {
        CommandResultEnvelope {
            command_id: command.command_id.clone(),
            status: CommandResultStatus::Completed,
            detail: None,
            reason_code: None,
            reason: None,
            required_capabilities: Vec::new(),
            accepted_at: None,
            by_identity: None,
            correlation_id: command.correlation_id.clone(),
            result,
        }
    }

    fn rejected_result(
        command: &MissionCommandEnvelope,
        reason_code: impl Into<String>,
        reason: impl Into<String>,
    ) -> CommandResultEnvelope {
        Self::rejected_result_with_capabilities(command, reason_code, reason, Vec::new())
    }

    fn rejected_result_with_capabilities(
        command: &MissionCommandEnvelope,
        reason_code: impl Into<String>,
        reason: impl Into<String>,
        required_capabilities: Vec<String>,
    ) -> CommandResultEnvelope {
        let reason = reason.into();
        CommandResultEnvelope {
            command_id: command.command_id.clone(),
            status: CommandResultStatus::Rejected,
            detail: Some(reason.clone()),
            reason_code: Some(reason_code.into()),
            reason: Some(reason),
            required_capabilities,
            accepted_at: None,
            by_identity: None,
            correlation_id: command.correlation_id.clone(),
            result: Value::Null,
        }
    }

    fn event_envelope_value(command: &MissionCommandEnvelope, event: &EventEnvelope) -> Value {
        json!({
            "event_id": Uuid::new_v4().simple().to_string(),
            "source": { "rns_identity": command.source.rns_identity },
            "timestamp": utc_now_rfc3339(),
            "event_type": event.event_type,
            "topics": command.topics,
            "payload": event.payload,
        })
    }

    fn rem_event_envelope_value(event: &EventEnvelope) -> Value {
        json!({
            "event_type": event.event_type,
            "payload": event.payload,
            "source": {
                "rns_identity": event.source.as_ref().map_or(Value::Null, |source| {
                    json!(source.rns_identity)
                })
            },
        })
    }

    pub fn ingest_protocol_envelope(
        &mut self,
        envelope: &ProtocolEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match &envelope.payload {
            Payload::Command(_) => {
                let rch_command =
                    r3akt_profile_rch::command_from_protocol(envelope, utc_now_rfc3339())
                        .map_err(|error| RchCoreError::InvalidPayload(error.to_string()))?;
                Ok(self.handle_command(&rch_command).event)
            }
            Payload::TopicMessage(TopicMessage {
                body,
                content_type,
                correlation_id: _,
                attachments: _,
            }) => {
                let topic_id = match &envelope.destination {
                    Destination::Topic(topic) => Some(topic.as_str().to_string()),
                    Destination::Node(_) | Destination::Broadcast => None,
                };
                let delivery = build_delivery_envelope(BuildDeliveryEnvelope {
                    sender: envelope.source.as_str().to_string(),
                    message_id: Some(envelope.id.to_string()),
                    topic_id: topic_id.clone(),
                    content_type: content_type.clone(),
                    ttl_seconds: envelope.ttl_seconds,
                    priority: DEFAULT_PRIORITY,
                    born_at_ms: Some(envelope.timestamp.timestamp() * 1000),
                    created_at: Some(
                        envelope
                            .timestamp
                            .to_rfc3339_opts(SecondsFormat::AutoSi, true),
                    ),
                })?;
                self.record_message(
                    delivery.message_id,
                    body,
                    topic_id.as_deref(),
                    None,
                    envelope.source.as_str(),
                )
                .map(Some)
            }
            Payload::NodeHello(_)
            | Payload::Heartbeat(_)
            | Payload::HealthTelemetry(_)
            | Payload::TelemetrySample(_)
            | Payload::AckAccepted(_)
            | Payload::AckRejected(_)
            | Payload::AckCompleted(_) => Ok(None),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn apply_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "rem.registry.mode.set"
            | "rem.registry.peers.list"
            | "rem.registry.team_peers.list" => self.apply_rem_registry_command(command),
            "mission.join"
            | "mission.leave"
            | "mission.pause"
            | "mission.resume"
            | "mission.nick"
            | "mission.users"
            | "mission.events.list" => self.apply_client_event_command(command),
            "mission.marker.list"
            | "mission.marker.create"
            | "mission.marker.position.patch"
            | "mission.marker.patch"
            | "mission.marker.delete" => self.apply_marker_command(command),
            "mission.zone.list"
            | "mission.zone.create"
            | "mission.zone.patch"
            | "mission.zone.delete" => self.apply_zone_command(command),
            "mission.registry.mission.upsert"
            | "mission.registry.mission.get"
            | "mission.registry.mission.list"
            | "mission.registry.mission.patch"
            | "mission.registry.mission.delete"
            | "mission.registry.mission.parent.set"
            | "mission.registry.mission.zone.link"
            | "mission.registry.mission.zone.unlink"
            | "mission.registry.mission.marker.link"
            | "mission.registry.mission.marker.unlink"
            | "mission.registry.mission.rde.set" => self.apply_registry_mission_command(command),
            "mission.registry.mission_change.upsert"
            | "mission.registry.mission_change.list"
            | "mission.registry.log_entry.upsert"
            | "mission.registry.log_entry.list" => self.apply_registry_log_command(command),
            "mission.registry.eam.list"
            | "mission.registry.eam.upsert"
            | "mission.registry.eam.get"
            | "mission.registry.eam.latest"
            | "mission.registry.eam.delete"
            | "mission.registry.eam.team.summary" => self.apply_registry_eam_command(command),
            "mission.registry.team.upsert"
            | "mission.registry.team.get"
            | "mission.registry.team.list"
            | "mission.registry.team.delete"
            | "mission.registry.team.mission.link"
            | "mission.registry.team.mission.unlink" => self.apply_registry_team_command(command),
            "mission.registry.team_member.upsert"
            | "mission.registry.team_member.get"
            | "mission.registry.team_member.list"
            | "mission.registry.team_member.delete"
            | "mission.registry.team_member.client.link"
            | "mission.registry.team_member.client.unlink" => {
                self.apply_registry_team_member_command(command)
            }
            "mission.registry.asset.upsert"
            | "mission.registry.asset.get"
            | "mission.registry.asset.list"
            | "mission.registry.asset.delete" => self.apply_registry_asset_command(command),
            "mission.registry.skill.upsert"
            | "mission.registry.skill.list"
            | "mission.registry.team_member_skill.upsert"
            | "mission.registry.team_member_skill.list"
            | "mission.registry.task_skill_requirement.upsert"
            | "mission.registry.task_skill_requirement.list" => {
                self.apply_registry_skill_command(command)
            }
            "mission.registry.assignment.upsert"
            | "mission.registry.assignment.list"
            | "mission.registry.assignment.asset.set"
            | "mission.registry.assignment.asset.link"
            | "mission.registry.assignment.asset.unlink" => {
                self.apply_registry_assignment_command(command)
            }
            "mission.registry.rights.subjects.list"
            | "mission.registry.rights.mission_access.assign"
            | "mission.registry.rights.mission_access.list"
            | "mission.registry.rights.mission_access.revoke" => {
                self.apply_registry_rights_command(command)
            }
            "checklist.template.list"
            | "checklist.template.get"
            | "checklist.template.create"
            | "checklist.template.update"
            | "checklist.template.clone"
            | "checklist.template.delete"
            | "checklist.create.online"
            | "checklist.create.offline"
            | "checklist.list.active"
            | "checklist.get"
            | "checklist.update"
            | "checklist.delete"
            | "checklist.join"
            | "checklist.upload"
            | "checklist.import.csv"
            | "checklist.feed.publish"
            | "checklist.task.row.add"
            | "checklist.task.row.delete"
            | "checklist.task.row.style.set"
            | "checklist.task.cell.set"
            | "checklist.task.status.set" => self.apply_checklist_command(command).map(Some),
            "ListTopic"
            | "topic.list"
            | "CreateTopic"
            | "topic.create"
            | "topic.patch"
            | "topic.delete"
            | "SubscribeTopic"
            | "CreateSubscriber"
            | "AddSubscriber"
            | "topic.subscribe"
            | "topic.subscriber.patch"
            | "topic.subscriber.delete"
            | "mission.message.send"
            | "PublishMessage" => self.apply_topic_command(command),
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_rem_registry_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "rem.registry.mode.set" => {
                let mode = optional_text_or_empty(&command.args, &["mode"]).unwrap_or_default();
                let record = self.set_identity_rem_mode(&command.source.rns_identity, mode)?;
                Ok(Some(Self::event(
                    "rem.registry.mode.updated",
                    command,
                    json!({
                        "identity": record.identity,
                        "mode": record.mode,
                        "effective_connected_mode": self.effective_rem_connected_mode(),
                        "registered_at_ms": record.updated_ts_ms,
                        "updated_at_ms": record.updated_ts_ms,
                    }),
                )))
            }
            "rem.registry.peers.list" => Ok(Some(Self::event(
                "rem.registry.peers.listed",
                command,
                self.rem_peer_registry_payload(),
            ))),
            "rem.registry.team_peers.list" => Ok(Some(Self::event(
                "rem.registry.team_peers.listed",
                command,
                self.rem_team_peer_registry_payload(&command.source.rns_identity),
            ))),
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn effective_rem_connected_mode(&self) -> bool {
        self.identity_rem_modes
            .values()
            .any(|record| record.mode.trim().eq_ignore_ascii_case("connected"))
    }

    fn rem_peer_registry_payload(&self) -> Value {
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
            if record.last_seen_ts_ms < cutoff_ms {
                continue;
            }
            if !record.client_type.trim().eq_ignore_ascii_case("rem") {
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
            "effective_connected_mode": self.effective_rem_connected_mode(),
            "items": items,
        })
    }

    fn apply_topic_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "ListTopic" | "topic.list" => Ok(Some(Self::event(
                "mission.topic.listed",
                command,
                json!({ "topics": self.topic_values() }),
            ))),
            "CreateTopic" | "topic.create" => {
                let topic_id = self.create_topic_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.topic.created",
                    command,
                    json!({ "topic_id": topic_id, "TopicID": topic_id }),
                )))
            }
            "topic.patch" => {
                let topic = self.patch_topic_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.topic.updated",
                    command,
                    Self::topic_value(&topic, self.subscribers(&topic.topic_id).len()),
                )))
            }
            "topic.delete" => {
                let topic = self.delete_topic_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.topic.deleted",
                    command,
                    Self::topic_value(&topic, 0),
                )))
            }
            "SubscribeTopic" | "CreateSubscriber" | "AddSubscriber" | "topic.subscribe" => {
                let (topic_id, subscriber_id, reject_tests, metadata) =
                    Self::subscription_from_args(command, &command.args)?;
                self.subscribe(&topic_id, &subscriber_id, reject_tests, metadata)?;
                Ok(Some(Self::event(
                    "mission.topic.subscribed",
                    command,
                    json!({
                        "topic_id": topic_id,
                        "TopicID": topic_id,
                        "subscriber_id": subscriber_id,
                        "Destination": subscriber_id,
                    }),
                )))
            }
            "topic.subscriber.patch" => {
                let subscriber = self.patch_subscriber_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.topic.subscriber.updated",
                    command,
                    Self::subscriber_value(&subscriber),
                )))
            }
            "topic.subscriber.delete" => {
                let subscriber = self.delete_subscriber_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.topic.subscriber.deleted",
                    command,
                    Self::subscriber_value(&subscriber),
                )))
            }
            "mission.message.send" | "PublishMessage" => {
                let content = required_text(&command.args, &["content", "Content"])?;
                let topic_id = optional_text(&command.args, &["topic_id", "TopicID", "topic_path"]);
                let destination = optional_text(&command.args, &["destination", "Destination"]);
                Ok(Some(self.record_message(
                    command.command_id.clone(),
                    &content,
                    topic_id.as_deref(),
                    destination.as_deref(),
                    &command.source.rns_identity,
                )?))
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_client_event_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.join" => {
                let identity = self.join_client(&command.source.rns_identity)?;
                Ok(Some(Self::event(
                    "mission.joined",
                    command,
                    json!({ "identity": identity, "joined": true }),
                )))
            }
            "mission.leave" => {
                let (identity, left) = self.leave_client(&command.source.rns_identity)?;
                Ok(Some(Self::event(
                    "mission.left",
                    command,
                    json!({ "identity": identity, "left": left }),
                )))
            }
            "mission.pause" => {
                let identity = self.pause_client(&command.source.rns_identity)?;
                Ok(Some(Self::event(
                    "mission.paused",
                    command,
                    json!({ "identity": identity, "paused": true }),
                )))
            }
            "mission.resume" => {
                let identity = self.resume_client(&command.source.rns_identity)?;
                Ok(Some(Self::event(
                    "mission.resumed",
                    command,
                    json!({ "identity": identity, "paused": false }),
                )))
            }
            "mission.nick" => {
                let nickname = required_text(&command.args, &["nickname", "nick", "name"])?;
                let (identity, nickname) =
                    self.set_client_nickname(&command.source.rns_identity, &nickname)?;
                Ok(Some(Self::event(
                    "mission.nickname.updated",
                    command,
                    json!({ "identity": identity, "nickname": nickname }),
                )))
            }
            "mission.users" => Ok(Some(Self::event(
                "mission.users.listed",
                command,
                json!({ "users": self.client_values() }),
            ))),
            "mission.events.list" => Ok(Some(Self::event(
                "mission.events.listed",
                command,
                json!({ "events": self.recent_audit_event_values(50) }),
            ))),
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_marker_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.marker.list" => Ok(Some(Self::event(
                "mission.marker.listed",
                command,
                json!({ "markers": self.marker_values() }),
            ))),
            "mission.marker.create" => {
                let marker = self.create_marker_from_args(command, &command.args)?;
                Ok(Some(Self::event(
                    "mission.marker.created",
                    command,
                    Self::marker_value(&marker),
                )))
            }
            "mission.marker.position.patch" => {
                let marker = self.patch_marker_position_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.marker.position.updated",
                    command,
                    Self::marker_value(&marker),
                )))
            }
            "mission.marker.patch" => {
                let marker = self.patch_marker_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.marker.updated",
                    command,
                    Self::marker_value(&marker),
                )))
            }
            "mission.marker.delete" => {
                let marker = self.delete_marker_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.marker.deleted",
                    command,
                    Self::marker_value(&marker),
                )))
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_zone_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.zone.list" => Ok(Some(Self::event(
                "mission.zone.listed",
                command,
                json!({ "zones": self.zone_values() }),
            ))),
            "mission.zone.create" => {
                let zone = self.create_zone_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.zone.created",
                    command,
                    Self::zone_value(&zone),
                )))
            }
            "mission.zone.patch" => {
                let zone = self.patch_zone_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.zone.updated",
                    command,
                    Self::zone_value(&zone),
                )))
            }
            "mission.zone.delete" => {
                let zone = self.delete_zone_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.zone.deleted",
                    command,
                    Self::zone_value(&zone),
                )))
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_registry_mission_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.registry.mission.upsert" => {
                let mission = self.upsert_mission_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.mission.upserted",
                    command,
                    self.mission_value_with_args(&mission, &command.args),
                )))
            }
            "mission.registry.mission.get" => {
                let mission_uid = required_text(&command.args, &["mission_uid", "uid"])?;
                let mission = self
                    .missions
                    .get(&mission_uid)
                    .ok_or(RchCoreError::MissionNotFound)?;
                Ok(Some(Self::event(
                    "mission.registry.mission.retrieved",
                    command,
                    self.mission_value_with_args(mission, &command.args),
                )))
            }
            "mission.registry.mission.list" => Ok(Some(Self::event(
                "mission.registry.mission.listed",
                command,
                json!({ "missions": self.limited_mission_values(&command.args) }),
            ))),
            "mission.registry.mission.patch" => {
                let mission = self.patch_mission_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.mission.updated",
                    command,
                    self.mission_value_with_args(&mission, &command.args),
                )))
            }
            "mission.registry.mission.delete" => {
                let mission = self.delete_mission_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.mission.deleted",
                    command,
                    self.mission_value(&mission),
                )))
            }
            "mission.registry.mission.parent.set" => {
                let mission = self.set_mission_parent_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.mission.parent.updated",
                    command,
                    self.mission_value(&mission),
                )))
            }
            "mission.registry.mission.zone.link" => {
                let mission_uid = required_text(&command.args, &["mission_uid"])?;
                let zone_id = required_text(&command.args, &["zone_id"])?;
                let mission = self.link_mission_zone(&mission_uid, &zone_id)?;
                Ok(Some(Self::event(
                    "mission.registry.mission.zone.linked",
                    command,
                    self.mission_value(&mission),
                )))
            }
            "mission.registry.mission.zone.unlink" => {
                let mission_uid = required_text(&command.args, &["mission_uid"])?;
                let zone_id = required_text(&command.args, &["zone_id"])?;
                let mission = self.unlink_mission_zone(&mission_uid, &zone_id)?;
                Ok(Some(Self::event(
                    "mission.registry.mission.zone.unlinked",
                    command,
                    self.mission_value(&mission),
                )))
            }
            "mission.registry.mission.marker.link" | "mission.registry.mission.marker.unlink" => {
                self.apply_registry_mission_marker_command(command)
            }
            "mission.registry.mission.rde.set" => {
                let mission_uid = required_text(&command.args, &["mission_uid"])?;
                let role = required_text(&command.args, &["role"])?;
                let payload = self.set_mission_rde(&mission_uid, &role)?;
                Ok(Some(Self::event(
                    "mission.registry.mission.rde.updated",
                    command,
                    payload,
                )))
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_registry_mission_marker_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        let mission_uid = required_text(&command.args, &["mission_uid"])?;
        let marker_id = required_text(&command.args, &["marker_id", "marker_ref"])?;
        let (event_type, mission) = match command.command_type.as_str() {
            "mission.registry.mission.marker.link" => (
                "mission.registry.mission.marker.linked",
                self.link_mission_marker(&mission_uid, &marker_id)?,
            ),
            "mission.registry.mission.marker.unlink" => (
                "mission.registry.mission.marker.unlinked",
                self.unlink_mission_marker(&mission_uid, &marker_id)?,
            ),
            other => return Err(RchCoreError::UnsupportedCommand(other.to_string())),
        };
        Ok(Some(Self::event(
            event_type,
            command,
            self.mission_value(&mission),
        )))
    }

    fn apply_registry_log_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.registry.mission_change.upsert" => {
                let change = self.upsert_mission_change_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.mission_change.upserted",
                    command,
                    Self::mission_change_value(&change),
                )))
            }
            "mission.registry.mission_change.list" => Ok(Some(Self::event(
                "mission.registry.mission_change.listed",
                command,
                json!({
                    "mission_changes": self.mission_change_values(
                        optional_text(&command.args, &["mission_uid", "mission_id"]).as_deref()
                    )
                }),
            ))),
            "mission.registry.log_entry.upsert" => {
                let entry = self.upsert_log_entry_from_args(command, &command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.log_entry.upserted",
                    command,
                    Self::log_entry_value(&entry),
                )))
            }
            "mission.registry.log_entry.list" => Ok(Some(Self::event(
                "mission.registry.log_entry.listed",
                command,
                json!({
                    "log_entries": self.log_entry_values(
                        optional_text(&command.args, &["mission_uid", "mission_id"]).as_deref(),
                        optional_text(&command.args, &["marker_ref"]).as_deref(),
                    )
                }),
            ))),
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_registry_eam_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        let (event_type, payload) = match command.command_type.as_str() {
            "mission.registry.eam.list" => (
                "mission.registry.eam.listed",
                json!({
                    "eams": self.eam_values(
                        optional_text(&command.args, &["team_uid"]).as_deref(),
                        optional_text(&command.args, &["overall_status"]).as_deref(),
                    )?
                }),
            ),
            "mission.registry.eam.upsert" => {
                let eam = self.upsert_eam_from_args(&command.args)?;
                (
                    "mission.registry.eam.upserted",
                    json!({ "eam": Self::eam_value(&eam) }),
                )
            }
            "mission.registry.eam.get" => {
                let callsign = required_text(&command.args, &["callsign"])?;
                let eam = self.active_eam_by_callsign(&callsign)?;
                (
                    "mission.registry.eam.retrieved",
                    json!({ "eam": Self::eam_value(eam) }),
                )
            }
            "mission.registry.eam.latest" => {
                let member = required_text(&command.args, &["team_member_uid"])?;
                let eam = self.latest_eam_by_member(&member)?;
                (
                    "mission.registry.eam.latest_retrieved",
                    json!({ "eam": Self::eam_value(eam) }),
                )
            }
            "mission.registry.eam.delete" => {
                let callsign = required_text(&command.args, &["callsign"])?;
                let eam = self.delete_eam_by_callsign(&callsign)?;
                (
                    "mission.registry.eam.deleted",
                    json!({ "eam": Self::eam_value(&eam) }),
                )
            }
            "mission.registry.eam.team.summary" => {
                let team_uid = required_text(&command.args, &["team_uid"])?;
                (
                    "mission.registry.eam.team_summary.retrieved",
                    json!({ "summary": self.eam_team_summary(&team_uid)? }),
                )
            }
            other => return Err(RchCoreError::UnsupportedCommand(other.to_string())),
        };
        Ok(Some(Self::event(event_type, command, payload)))
    }

    fn apply_registry_team_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.registry.team.upsert" => {
                let team = self.upsert_team_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.team.upserted",
                    command,
                    Self::team_value(&team),
                )))
            }
            "mission.registry.team.get" => {
                let team_uid = required_text(&command.args, &["team_uid", "uid"])?;
                let team = self
                    .teams
                    .get(&team_uid)
                    .ok_or(RchCoreError::TeamNotFound)?;
                Ok(Some(Self::event(
                    "mission.registry.team.retrieved",
                    command,
                    Self::team_value(team),
                )))
            }
            "mission.registry.team.list" => Ok(Some(Self::event(
                "mission.registry.team.listed",
                command,
                json!({
                    "teams": self.team_values(optional_text(&command.args, &["mission_uid"]).as_deref())
                }),
            ))),
            "mission.registry.team.delete" => {
                let team = self.delete_team_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.team.deleted",
                    command,
                    Self::team_value(&team),
                )))
            }
            "mission.registry.team.mission.link" => {
                let team = self.link_team_mission_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.team.mission.linked",
                    command,
                    Self::team_value(&team),
                )))
            }
            "mission.registry.team.mission.unlink" => {
                let team = self.unlink_team_mission_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.team.mission.unlinked",
                    command,
                    Self::team_value(&team),
                )))
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_registry_team_member_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.registry.team_member.upsert" => {
                let member = self.upsert_team_member_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.team_member.upserted",
                    command,
                    Self::team_member_value(&member),
                )))
            }
            "mission.registry.team_member.get" => {
                let uid = required_text(&command.args, &["team_member_uid", "uid"])?;
                let member = self
                    .team_members
                    .get(&uid)
                    .ok_or(RchCoreError::TeamMemberNotFound)?;
                Ok(Some(Self::event(
                    "mission.registry.team_member.retrieved",
                    command,
                    Self::team_member_value(member),
                )))
            }
            "mission.registry.team_member.list" => Ok(Some(Self::event(
                "mission.registry.team_member.listed",
                command,
                json!({
                    "team_members": self.team_member_values(
                        optional_text(&command.args, &["team_uid"]).as_deref()
                    )
                }),
            ))),
            "mission.registry.team_member.delete" => {
                let member = self.delete_team_member_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.team_member.deleted",
                    command,
                    Self::team_member_value(&member),
                )))
            }
            "mission.registry.team_member.client.link" => {
                let member = self.link_team_member_client_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.team_member.client.linked",
                    command,
                    Self::team_member_value(&member),
                )))
            }
            "mission.registry.team_member.client.unlink" => {
                let member = self.unlink_team_member_client_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.team_member.client.unlinked",
                    command,
                    Self::team_member_value(&member),
                )))
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_registry_asset_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.registry.asset.upsert" => {
                let previous_team_member_uid = optional_text(&command.args, &["asset_uid"])
                    .and_then(|asset_uid| self.assets.get(&asset_uid))
                    .and_then(|asset| asset.team_member_uid.clone());
                let asset = self.upsert_asset_from_args(&command.args)?;
                let mission_uids =
                    self.mission_uids_for_asset_upsert(&asset, previous_team_member_uid);
                self.emit_auto_asset_mission_changes(
                    command,
                    mission_uids,
                    "mission.asset.upserted",
                    "ADD_CONTENT",
                    &asset,
                );
                Ok(Some(Self::event(
                    "mission.registry.asset.upserted",
                    command,
                    Self::asset_value(&asset),
                )))
            }
            "mission.registry.asset.get" => {
                let asset_uid = required_text(&command.args, &["asset_uid"])?;
                let asset = self
                    .assets
                    .get(&asset_uid)
                    .ok_or(RchCoreError::AssetNotFound)?;
                Ok(Some(Self::event(
                    "mission.registry.asset.retrieved",
                    command,
                    Self::asset_value(asset),
                )))
            }
            "mission.registry.asset.list" => Ok(Some(Self::event(
                "mission.registry.asset.listed",
                command,
                json!({
                    "assets": self.asset_values(
                        optional_text(&command.args, &["team_member_uid"]).as_deref()
                    )
                }),
            ))),
            "mission.registry.asset.delete" => {
                let asset_uid = required_text(&command.args, &["asset_uid"])?;
                let asset = self
                    .assets
                    .get(&asset_uid)
                    .cloned()
                    .ok_or(RchCoreError::AssetNotFound)?;
                let mission_uids = self.mission_uids_for_asset_delete(&asset_uid, &asset);
                let asset = self.delete_asset(&asset_uid)?;
                self.emit_auto_asset_mission_changes(
                    command,
                    mission_uids,
                    "mission.asset.deleted",
                    "REMOVE_CONTENT",
                    &asset,
                );
                Ok(Some(Self::event(
                    "mission.registry.asset.deleted",
                    command,
                    Self::asset_value(&asset),
                )))
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn emit_auto_asset_mission_changes(
        &mut self,
        command: &MissionCommandEnvelope,
        mission_uids: Vec<String>,
        source_event_type: &str,
        change_type: &str,
        asset: &AssetRecord,
    ) {
        let asset_delta = asset_mission_delta(asset, source_event_type);
        for mission_uid in mission_uids {
            if !self.missions.contains_key(&mission_uid) {
                continue;
            }
            let now = utc_now_ms();
            let change = MissionChangeRecord {
                uid: Uuid::new_v4().simple().to_string(),
                mission_uid,
                name: Some(source_event_type.to_string()),
                team_member_rns_identity: None,
                timestamp_ms: now,
                notes: None,
                change_type: change_type.to_string(),
                is_federated_change: false,
                hashes: Vec::new(),
                delta: json!({
                    "version": 1,
                    "contract_version": "r3akt.mission.change.v1",
                    "source_event_type": source_event_type,
                    "emitted_at": millis_to_rfc3339(now),
                    "logs": [],
                    "assets": [asset_delta],
                    "tasks": [],
                    "checklists": [],
                }),
            };
            self.mission_changes
                .insert(change.uid.clone(), change.clone());
            let event = Self::event(
                "mission.registry.mission_change.upserted",
                command,
                Self::mission_change_value(&change),
            );
            self.record_audit_event(command, &event);
        }
    }

    fn apply_registry_skill_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.registry.skill.upsert" => {
                let skill = self.upsert_skill_from_args(&command.args);
                Ok(Some(Self::event(
                    "mission.registry.skill.upserted",
                    command,
                    Self::skill_value(&skill),
                )))
            }
            "mission.registry.skill.list" => Ok(Some(Self::event(
                "mission.registry.skill.listed",
                command,
                json!({ "skills": self.skill_values() }),
            ))),
            "mission.registry.team_member_skill.upsert" => {
                let member_skill = self.upsert_team_member_skill_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.team_member_skill.upserted",
                    command,
                    Self::team_member_skill_value(&member_skill),
                )))
            }
            "mission.registry.team_member_skill.list" => Ok(Some(Self::event(
                "mission.registry.team_member_skill.listed",
                command,
                json!({
                    "team_member_skills": self.team_member_skill_values(
                        optional_text(&command.args, &["team_member_rns_identity"]).as_deref()
                    )
                }),
            ))),
            "mission.registry.task_skill_requirement.upsert" => {
                let requirement = self.upsert_task_skill_requirement_from_args(&command.args)?;
                Ok(Some(Self::event(
                    "mission.registry.task_skill_requirement.upserted",
                    command,
                    Self::task_skill_requirement_value(&requirement),
                )))
            }
            "mission.registry.task_skill_requirement.list" => Ok(Some(Self::event(
                "mission.registry.task_skill_requirement.listed",
                command,
                json!({
                    "task_skill_requirements": self.task_skill_requirement_values(
                        optional_text(&command.args, &["task_uid"]).as_deref()
                    )
                }),
            ))),
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_registry_assignment_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.registry.assignment.upsert" => {
                let assignment = self.upsert_assignment_from_args(&command.args)?;
                self.emit_auto_assignment_mission_change(
                    command,
                    &assignment,
                    "mission.assignment.upserted",
                    "assignment_upsert",
                    "ADD_CONTENT",
                    None,
                )?;
                Ok(Some(Self::event(
                    "mission.registry.assignment.upserted",
                    command,
                    self.assignment_value(&assignment),
                )))
            }
            "mission.registry.assignment.list" => Ok(Some(Self::event(
                "mission.registry.assignment.listed",
                command,
                json!({
                    "assignments": self.assignment_values(
                        optional_text(&command.args, &["mission_uid", "mission_id"]).as_deref(),
                        optional_text(&command.args, &["task_uid"]).as_deref()
                    )
                }),
            ))),
            "mission.registry.assignment.asset.set" => {
                let assignment_uid = required_text(&command.args, &["assignment_uid"])?;
                let assets = string_list(command.args.get("assets"), "assets")?;
                let assignment = self.set_assignment_assets(&assignment_uid, assets)?;
                self.emit_auto_assignment_mission_change(
                    command,
                    &assignment,
                    "mission.assignment.assets.updated",
                    "assignment_assets_set",
                    "ADD_CONTENT",
                    None,
                )?;
                Ok(Some(Self::event(
                    "mission.registry.assignment.asset.set",
                    command,
                    self.assignment_value(&assignment),
                )))
            }
            "mission.registry.assignment.asset.link" => {
                let assignment_uid = required_text(&command.args, &["assignment_uid"])?;
                let asset_uid = required_text(&command.args, &["asset_uid"])?;
                let assignment = self.link_assignment_asset(&assignment_uid, &asset_uid)?;
                self.emit_auto_assignment_mission_change(
                    command,
                    &assignment,
                    "mission.assignment.asset.linked",
                    "assignment_asset_linked",
                    "ADD_CONTENT",
                    Some(asset_uid),
                )?;
                Ok(Some(Self::event(
                    "mission.registry.assignment.asset.linked",
                    command,
                    self.assignment_value(&assignment),
                )))
            }
            "mission.registry.assignment.asset.unlink" => {
                let assignment_uid = required_text(&command.args, &["assignment_uid"])?;
                let asset_uid = required_text(&command.args, &["asset_uid"])?;
                let assignment = self.unlink_assignment_asset(&assignment_uid, &asset_uid)?;
                self.emit_auto_assignment_mission_change(
                    command,
                    &assignment,
                    "mission.assignment.asset.unlinked",
                    "assignment_asset_unlinked",
                    "REMOVE_CONTENT",
                    Some(asset_uid),
                )?;
                Ok(Some(Self::event(
                    "mission.registry.assignment.asset.unlinked",
                    command,
                    self.assignment_value(&assignment),
                )))
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn emit_auto_assignment_mission_change(
        &mut self,
        command: &MissionCommandEnvelope,
        assignment: &AssignmentRecord,
        source_event_type: &str,
        op: &str,
        change_type: &str,
        asset_uid: Option<String>,
    ) -> Result<(), RchCoreError> {
        if !self.missions.contains_key(&assignment.mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        let now = utc_now_ms();
        let task_delta = assignment_mission_delta(self, assignment, op, asset_uid);
        let change = MissionChangeRecord {
            uid: Uuid::new_v4().simple().to_string(),
            mission_uid: assignment.mission_uid.clone(),
            name: Some(source_event_type.to_string()),
            team_member_rns_identity: Some(assignment.team_member_rns_identity.clone()),
            timestamp_ms: now,
            notes: None,
            change_type: change_type.to_string(),
            is_federated_change: false,
            hashes: Vec::new(),
            delta: json!({
                "version": 1,
                "contract_version": "r3akt.mission.change.v1",
                "source_event_type": source_event_type,
                "emitted_at": millis_to_rfc3339(now),
                "logs": [],
                "assets": [],
                "tasks": [task_delta],
                "checklists": [],
            }),
        };
        self.mission_changes
            .insert(change.uid.clone(), change.clone());
        let event = Self::event(
            "mission.registry.mission_change.upserted",
            command,
            Self::mission_change_value(&change),
        );
        self.record_audit_event(command, &event);
        Ok(())
    }

    fn apply_registry_rights_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Option<EventEnvelope>, RchCoreError> {
        match command.command_type.as_str() {
            "mission.registry.rights.subjects.list" => Ok(Some(Self::event(
                "mission.registry.rights.subjects.listed",
                command,
                json!({
                    "subjects": self.team_member_subject_values(
                        optional_text(&command.args, &["mission_uid"]).as_deref()
                    )
                }),
            ))),
            "mission.registry.rights.mission_access.assign" => {
                let mission_uid = required_text(&command.args, &["mission_uid"])?;
                if !self.missions.contains_key(&mission_uid) {
                    return Err(RchCoreError::MissionNotFound);
                }
                let raw_subject_type = required_text(&command.args, &["subject_type"])?;
                let subject_type = normalize_subject_type(&raw_subject_type)?;
                let raw_subject_id = required_text(&command.args, &["subject_id"])?;
                let subject_id = normalize_subject_id(&subject_type, &raw_subject_id)?;
                if subject_type == "team_member" && !self.team_members.contains_key(&subject_id) {
                    return Err(RchCoreError::TeamMemberNotFound);
                }
                let role = optional_text(&command.args, &["role"])
                    .or_else(|| {
                        self.missions
                            .get(&mission_uid)
                            .and_then(|mission| mission.default_role.clone())
                    })
                    .unwrap_or_else(|| "MISSION_SUBSCRIBER".to_string());
                self.assign_mission_access_role(&mission_uid, &subject_type, &subject_id, &role)?;
                let assignment = self
                    .mission_access_assignments
                    .get(&(mission_uid.clone(), subject_type, subject_id))
                    .ok_or(RchCoreError::AssignmentNotFound)?;
                Ok(Some(Self::event(
                    "mission.registry.rights.mission_access.assigned",
                    command,
                    Self::mission_access_assignment_value(assignment),
                )))
            }
            "mission.registry.rights.mission_access.list" => Ok(Some(Self::event(
                "mission.registry.rights.mission_access.listed",
                command,
                json!({
                    "mission_access_assignments": self.mission_access_assignment_values(
                        optional_text(&command.args, &["mission_uid"]).as_deref(),
                        optional_text(&command.args, &["subject_type"]).as_deref(),
                        optional_text(&command.args, &["subject_id"]).as_deref(),
                    )
                }),
            ))),
            "mission.registry.rights.mission_access.revoke" => {
                let mission_uid = required_text(&command.args, &["mission_uid"])?;
                let subject_type =
                    normalize_subject_type(&required_text(&command.args, &["subject_type"])?)?;
                let subject_id = normalize_subject_id(
                    &subject_type,
                    &required_text(&command.args, &["subject_id"])?,
                )?;
                let deleted = self
                    .mission_access_assignments
                    .remove(&(
                        mission_uid.clone(),
                        subject_type.clone(),
                        subject_id.clone(),
                    ))
                    .is_some();
                Ok(Some(Self::event(
                    "mission.registry.rights.mission_access.revoked",
                    command,
                    json!({
                        "mission_uid": mission_uid,
                        "subject_type": subject_type,
                        "subject_id": subject_id,
                        "deleted": deleted,
                    }),
                )))
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn apply_checklist_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<EventEnvelope, RchCoreError> {
        let previous_task_status = if command.command_type == "checklist.task.status.set" {
            optional_text(&command.args, &["task_uid"]).and_then(|task_uid| {
                self.checklist_tasks
                    .get(&task_uid)
                    .map(|task| task.task_status.clone())
            })
        } else {
            None
        };
        let deleted_task_delta = if command.command_type == "checklist.task.row.delete" {
            self.checklist_task_row_deleted_delta(&command.args)
        } else {
            None
        };
        let row_add_updates_existing = if command.command_type == "checklist.task.row.add" {
            optional_text(&command.args, &["checklist_uid"])
                .zip(optional_text(&command.args, &["task_uid"]))
                .is_some_and(|(checklist_uid, task_uid)| {
                    self.checklist_tasks
                        .get(&task_uid)
                        .is_some_and(|task| task.checklist_uid == checklist_uid)
                })
        } else {
            false
        };
        let checklist_update_previous_mission_uid = if command.command_type == "checklist.update" {
            optional_text(&command.args, &["checklist_uid"])
                .and_then(|checklist_uid| self.checklists.get(&checklist_uid).cloned())
                .and_then(|checklist| checklist.mission_uid.and_then(none_if_empty))
        } else {
            None
        };
        let payload = match command.command_type.as_str() {
            "checklist.template.list"
            | "checklist.template.get"
            | "checklist.template.create"
            | "checklist.template.update"
            | "checklist.template.clone"
            | "checklist.template.delete" => self.apply_checklist_template_command(command)?,
            "checklist.create.online" => {
                let checklist = self.create_checklist_from_args(command, "ONLINE", "SYNCED")?;
                self.checklist_value(&checklist)
            }
            "checklist.create.offline" => {
                let checklist =
                    self.create_checklist_from_args(command, "OFFLINE", "LOCAL_ONLY")?;
                self.checklist_value(&checklist)
            }
            "checklist.list.active" => json!({ "checklists": self.checklist_values() }),
            "checklist.get" => {
                let checklist_uid = required_text(&command.args, &["checklist_uid"])?;
                let checklist = self.checklists.get(&checklist_uid).ok_or_else(|| {
                    RchCoreError::InvalidPayload(format!("Checklist '{checklist_uid}' not found"))
                })?;
                self.checklist_value(checklist)
            }
            "checklist.update" => {
                let checklist_uid = required_text(&command.args, &["checklist_uid"])?;
                let patch = command.args.get("patch").ok_or_else(|| {
                    RchCoreError::InvalidPayload("checklist_uid and patch are required".to_string())
                })?;
                let checklist = self.update_checklist_from_patch(&checklist_uid, patch)?;
                self.checklist_value(&checklist)
            }
            "checklist.delete" => {
                let checklist_uid = required_text(&command.args, &["checklist_uid"])?;
                self.delete_checklist(&checklist_uid)?
            }
            "checklist.join" => {
                let checklist_uid = required_text(&command.args, &["checklist_uid"])?;
                let checklist = self.checklists.get(&checklist_uid).ok_or_else(|| {
                    RchCoreError::InvalidPayload(format!("Checklist '{checklist_uid}' not found"))
                })?;
                self.checklist_value(checklist)
            }
            "checklist.upload" => {
                let checklist_uid = required_text(&command.args, &["checklist_uid"])?;
                let checklist = self.upload_checklist(&checklist_uid)?;
                self.checklist_value(&checklist)
            }
            "checklist.import.csv" => self.import_checklist_csv(command)?,
            "checklist.feed.publish" => {
                let checklist_uid = required_text(&command.args, &["checklist_uid"])?;
                let mission_feed_uid = required_text(&command.args, &["mission_feed_uid"])?;
                let publication =
                    self.publish_checklist_feed(&checklist_uid, &mission_feed_uid, command)?;
                checklist_feed_publication_value(&publication)
            }
            "checklist.task.row.add"
            | "checklist.task.row.delete"
            | "checklist.task.row.style.set"
            | "checklist.task.cell.set"
            | "checklist.task.status.set" => self.apply_checklist_task_command(command)?,
            other => return Err(RchCoreError::UnsupportedCommand(other.to_string())),
        };
        let event_type = checklist_event_type(command.command_type.as_str());
        if matches!(
            command.command_type.as_str(),
            "checklist.create.online" | "checklist.import.csv"
        ) {
            self.emit_auto_checklist_created_mission_change(command, &payload)?;
        } else if matches!(command.command_type.as_str(), "checklist.upload") {
            self.emit_auto_checklist_uploaded_mission_change(command, &payload)?;
        } else if command.command_type == "checklist.update"
            && checklist_update_added_shareable_mission(
                checklist_update_previous_mission_uid.as_deref(),
                &payload,
            )
        {
            self.emit_auto_checklist_created_mission_change(command, &payload)?;
        } else if command.command_type == "checklist.task.row.add" && !row_add_updates_existing {
            self.emit_auto_checklist_task_row_added_mission_change(command, &payload)?;
        } else if command.command_type == "checklist.task.row.style.set" {
            self.emit_auto_checklist_task_mutation_mission_change(
                command,
                &payload,
                "mission.checklist.task.row.style_set",
                checklist_task_row_style_delta(&payload, &command.args),
                None,
                "ADD_CONTENT",
            )?;
        } else if command.command_type == "checklist.task.cell.set" {
            let (delta, team_member_rns_identity) =
                checklist_task_cell_set_delta(&payload, &command.args);
            self.emit_auto_checklist_task_mutation_mission_change(
                command,
                &payload,
                "mission.checklist.task.cell_set",
                delta,
                team_member_rns_identity,
                "ADD_CONTENT",
            )?;
        } else if command.command_type == "checklist.task.status.set" {
            let (delta, team_member_rns_identity) = checklist_task_status_set_delta(
                &payload,
                &command.args,
                previous_task_status.as_ref(),
            );
            self.emit_auto_checklist_task_mutation_mission_change(
                command,
                &payload,
                "mission.checklist.task.status_set",
                delta,
                team_member_rns_identity,
                "ADD_CONTENT",
            )?;
        } else if command.command_type == "checklist.task.row.delete" {
            self.emit_auto_checklist_task_mutation_mission_change(
                command,
                &payload,
                "mission.checklist.task.row.deleted",
                deleted_task_delta,
                None,
                "REMOVE_CONTENT",
            )?;
        }
        Ok(Self::event(event_type, command, payload))
    }

    fn emit_auto_checklist_created_mission_change(
        &mut self,
        command: &MissionCommandEnvelope,
        checklist_payload: &Value,
    ) -> Result<(), RchCoreError> {
        let mission_uid =
            optional_text(checklist_payload, &["mission_uid", "mission_id"]).unwrap_or_default();
        if !mission_uid.is_empty() && !self.missions.contains_key(&mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        let now = utc_now_ms();
        let source_event_type = "mission.checklist.created";
        let change = MissionChangeRecord {
            uid: Uuid::new_v4().simple().to_string(),
            mission_uid,
            name: Some(source_event_type.to_string()),
            team_member_rns_identity: optional_text_or_empty(
                checklist_payload,
                &["created_by_team_member_rns_identity"],
            ),
            timestamp_ms: now,
            notes: None,
            change_type: "ADD_CONTENT".to_string(),
            is_federated_change: false,
            hashes: Vec::new(),
            delta: json!({
                "version": 1,
                "contract_version": "r3akt.mission.change.v1",
                "source_event_type": source_event_type,
                "emitted_at": millis_to_rfc3339(now),
                "logs": [],
                "assets": [],
                "tasks": [],
                "checklists": [checklist_payload],
            }),
        };
        self.mission_changes
            .insert(change.uid.clone(), change.clone());
        let event = Self::event(
            "mission.registry.mission_change.upserted",
            command,
            Self::mission_change_value(&change),
        );
        self.record_audit_event(command, &event);
        Ok(())
    }

    fn emit_auto_checklist_uploaded_mission_change(
        &mut self,
        command: &MissionCommandEnvelope,
        checklist_payload: &Value,
    ) -> Result<(), RchCoreError> {
        let mission_uid =
            optional_text(checklist_payload, &["mission_uid", "mission_id"]).unwrap_or_default();
        if !mission_uid.is_empty() && !self.missions.contains_key(&mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        let now = utc_now_ms();
        let source_event_type = "mission.checklist.uploaded";
        let change = MissionChangeRecord {
            uid: Uuid::new_v4().simple().to_string(),
            mission_uid,
            name: Some(source_event_type.to_string()),
            team_member_rns_identity: optional_text_or_empty(&command.args, &["source_identity"])
                .and_then(none_if_empty),
            timestamp_ms: now,
            notes: None,
            change_type: "ADD_CONTENT".to_string(),
            is_federated_change: false,
            hashes: Vec::new(),
            delta: json!({
                "version": 1,
                "contract_version": "r3akt.mission.change.v1",
                "source_event_type": source_event_type,
                "emitted_at": millis_to_rfc3339(now),
                "logs": [],
                "assets": [],
                "tasks": [],
                "checklists": [checklist_payload],
            }),
        };
        self.mission_changes
            .insert(change.uid.clone(), change.clone());
        let event = Self::event(
            "mission.registry.mission_change.upserted",
            command,
            Self::mission_change_value(&change),
        );
        self.record_audit_event(command, &event);
        Ok(())
    }

    fn emit_auto_checklist_task_row_added_mission_change(
        &mut self,
        command: &MissionCommandEnvelope,
        checklist_payload: &Value,
    ) -> Result<(), RchCoreError> {
        let mission_uid =
            optional_text(checklist_payload, &["mission_uid", "mission_id"]).unwrap_or_default();
        if !mission_uid.is_empty() && !self.missions.contains_key(&mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        let Some(task_delta) = checklist_task_row_added_delta(checklist_payload, &command.args)
        else {
            return Ok(());
        };
        let now = utc_now_ms();
        let source_event_type = "mission.checklist.task.row.added";
        let change = MissionChangeRecord {
            uid: Uuid::new_v4().simple().to_string(),
            mission_uid,
            name: Some(source_event_type.to_string()),
            team_member_rns_identity: None,
            timestamp_ms: now,
            notes: None,
            change_type: "ADD_CONTENT".to_string(),
            is_federated_change: false,
            hashes: Vec::new(),
            delta: json!({
                "version": 1,
                "contract_version": "r3akt.mission.change.v1",
                "source_event_type": source_event_type,
                "emitted_at": millis_to_rfc3339(now),
                "logs": [],
                "assets": [],
                "tasks": [task_delta],
                "checklists": [],
            }),
        };
        self.mission_changes
            .insert(change.uid.clone(), change.clone());
        let event = Self::event(
            "mission.registry.mission_change.upserted",
            command,
            Self::mission_change_value(&change),
        );
        self.record_audit_event(command, &event);
        Ok(())
    }

    fn emit_auto_checklist_task_mutation_mission_change(
        &mut self,
        command: &MissionCommandEnvelope,
        checklist_payload: &Value,
        source_event_type: &str,
        task_delta: Option<Value>,
        team_member_rns_identity: Option<String>,
        change_type: &str,
    ) -> Result<(), RchCoreError> {
        let mission_uid =
            optional_text(checklist_payload, &["mission_uid", "mission_id"]).unwrap_or_default();
        if !mission_uid.is_empty() && !self.missions.contains_key(&mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        let Some(task_delta) = task_delta else {
            return Ok(());
        };
        let now = utc_now_ms();
        let change = MissionChangeRecord {
            uid: Uuid::new_v4().simple().to_string(),
            mission_uid,
            name: Some(source_event_type.to_string()),
            team_member_rns_identity,
            timestamp_ms: now,
            notes: None,
            change_type: change_type.to_string(),
            is_federated_change: false,
            hashes: Vec::new(),
            delta: json!({
                "version": 1,
                "contract_version": "r3akt.mission.change.v1",
                "source_event_type": source_event_type,
                "emitted_at": millis_to_rfc3339(now),
                "logs": [],
                "assets": [],
                "tasks": [task_delta],
                "checklists": [],
            }),
        };
        self.mission_changes
            .insert(change.uid.clone(), change.clone());
        let event = Self::event(
            "mission.registry.mission_change.upserted",
            command,
            Self::mission_change_value(&change),
        );
        self.record_audit_event(command, &event);
        Ok(())
    }

    fn checklist_task_row_deleted_delta(&self, args: &Value) -> Option<Value> {
        let checklist_uid = optional_text(args, &["checklist_uid"])?;
        let task_uid = optional_text(args, &["task_uid"])?;
        let checklist = self.checklists.get(&checklist_uid)?;
        let task = self
            .checklist_tasks
            .get(&task_uid)
            .filter(|task| task.checklist_uid == checklist_uid)?;
        Some(json!({
            "op": "row_deleted",
            "mission_uid": checklist.mission_uid.clone(),
            "checklist_uid": checklist_uid,
            "task_uid": task.task_uid.clone(),
            "number": task.number,
            "status": task.task_status.clone(),
            "user_status": task.user_status.clone(),
        }))
    }

    fn apply_checklist_template_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Value, RchCoreError> {
        match command.command_type.as_str() {
            "checklist.template.list" => {
                Ok(json!({ "templates": self.checklist_template_values() }))
            }
            "checklist.template.get" => {
                let template_uid = required_text(&command.args, &["template_uid"])?;
                let template = self.checklist_templates.get(&template_uid).ok_or_else(|| {
                    RchCoreError::InvalidPayload(format!(
                        "Checklist template '{template_uid}' not found"
                    ))
                })?;
                Ok(self.checklist_template_value(template))
            }
            "checklist.template.create" => {
                let template = command.args.get("template").ok_or_else(|| {
                    RchCoreError::InvalidPayload("template is required".to_string())
                })?;
                let template = self.create_checklist_template(template, None)?;
                Ok(self.checklist_template_value(&template))
            }
            "checklist.template.update" => {
                let template_uid = required_text(&command.args, &["template_uid"])?;
                let patch = command.args.get("patch").ok_or_else(|| {
                    RchCoreError::InvalidPayload("template_uid and patch are required".to_string())
                })?;
                let template = self.update_checklist_template(&template_uid, patch)?;
                Ok(self.checklist_template_value(&template))
            }
            "checklist.template.clone" => {
                let source_uid = required_text(&command.args, &["source_template_uid"])?;
                let name = required_text(&command.args, &["template_name"])?;
                let template = self.clone_checklist_template(command, &source_uid, &name)?;
                Ok(self.checklist_template_value(&template))
            }
            "checklist.template.delete" => {
                let template_uid = required_text(&command.args, &["template_uid"])?;
                self.delete_checklist_template(&template_uid)
            }
            other => Err(RchCoreError::UnsupportedCommand(other.to_string())),
        }
    }

    fn apply_checklist_task_command(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Value, RchCoreError> {
        let checklist_uid = required_text(&command.args, &["checklist_uid"])?;
        let checklist = match command.command_type.as_str() {
            "checklist.task.row.add" => {
                self.add_checklist_task_row(&checklist_uid, &command.args)?
            }
            "checklist.task.row.delete" => {
                let task_uid = required_text(&command.args, &["task_uid"])?;
                self.delete_checklist_task_row(&checklist_uid, &task_uid)?
            }
            "checklist.task.row.style.set" => {
                let task_uid = required_text(&command.args, &["task_uid"])?;
                self.set_checklist_task_row_style(&checklist_uid, &task_uid, &command.args)?
            }
            "checklist.task.cell.set" => {
                let task_uid = required_text(&command.args, &["task_uid"])?;
                let column_uid = required_text(&command.args, &["column_uid"])?;
                self.set_checklist_task_cell(&checklist_uid, &task_uid, &column_uid, &command.args)?
            }
            "checklist.task.status.set" => {
                let task_uid = required_text(&command.args, &["task_uid"])?;
                self.set_checklist_task_status(&checklist_uid, &task_uid, &command.args)?
            }
            other => return Err(RchCoreError::UnsupportedCommand(other.to_string())),
        };
        Ok(self.checklist_value(&checklist))
    }

    fn join_client(&mut self, identity: &str) -> Result<String, RchCoreError> {
        let display_identity = identity.trim().to_string();
        let identity = normalize_hash(Some(identity))
            .ok_or_else(|| RchCoreError::InvalidPayload("identity is required".to_string()))?;
        if self.identity_blocked(&identity) {
            return Err(RchCoreError::Forbidden(
                "identity is banned or blackholed".to_string(),
            ));
        }
        let now = utc_now_ms();
        self.clients
            .entry(identity.clone())
            .and_modify(|client| {
                client.identity.clone_from(&display_identity);
                client.last_seen_ts_ms = now;
            })
            .or_insert_with(|| ClientRecord {
                identity: display_identity,
                first_seen_ts_ms: now,
                last_seen_ts_ms: now,
                nickname: None,
                role: DEFAULT_ROSTER_ROLE.to_string(),
                paused: false,
                text_only: false,
                last_chat_ts_ms: None,
            });
        Ok(identity)
    }

    fn leave_client(&mut self, identity: &str) -> Result<(String, bool), RchCoreError> {
        let identity = normalize_hash(Some(identity))
            .ok_or_else(|| RchCoreError::InvalidPayload("identity is required".to_string()))?;
        let left = self.clients.remove(&identity).is_some();
        Ok((identity, left))
    }

    fn pause_client(&mut self, identity: &str) -> Result<String, RchCoreError> {
        let identity = self.ensure_roster_client(identity)?;
        if let Some(client) = self.clients.get_mut(&identity) {
            client.paused = true;
            client.last_seen_ts_ms = utc_now_ms();
        }
        Ok(identity)
    }

    fn resume_client(&mut self, identity: &str) -> Result<String, RchCoreError> {
        let identity = self.ensure_roster_client(identity)?;
        if let Some(client) = self.clients.get_mut(&identity) {
            client.paused = false;
            client.last_seen_ts_ms = utc_now_ms();
        }
        Ok(identity)
    }

    fn set_client_nickname(
        &mut self,
        identity: &str,
        nickname: &str,
    ) -> Result<(String, String), RchCoreError> {
        let identity = self.ensure_roster_client(identity)?;
        let nickname = normalize_roster_nickname(nickname)?;
        if let Some(client) = self.clients.get_mut(&identity) {
            client.nickname = Some(nickname.clone());
            client.last_seen_ts_ms = utc_now_ms();
        }
        Ok((identity, nickname))
    }

    fn ensure_roster_client(&mut self, identity: &str) -> Result<String, RchCoreError> {
        let normalized = normalize_hash(Some(identity))
            .ok_or_else(|| RchCoreError::InvalidPayload("identity is required".to_string()))?;
        if !self.clients.contains_key(&normalized) {
            self.join_client(identity)?;
        }
        Ok(normalized)
    }

    fn identity_blocked(&self, identity: &str) -> bool {
        self.identity_states
            .get(identity)
            .is_some_and(|state| state.is_banned || state.is_blackholed)
    }

    fn create_marker_from_args(
        &mut self,
        command: &MissionCommandEnvelope,
        args: &Value,
    ) -> Result<MarkerRecord, RchCoreError> {
        let lat = required_f64(args, "lat")?;
        let lon = required_f64(args, "lon")?;
        let local_id = Uuid::new_v4().simple().to_string();
        let object_destination_hash = Uuid::new_v4().simple().to_string();
        let now = utc_now_ms();
        let marker_type = normalize_marker_symbol(
            &optional_text(args, &["marker_type", "type"]).unwrap_or_else(|| "marker".to_string()),
        )
        .unwrap_or_else(|| "marker".to_string());
        if !is_supported_marker_symbol(&marker_type) {
            return Err(RchCoreError::InvalidPayload(
                "Unsupported marker type".to_string(),
            ));
        }
        let symbol = normalize_marker_symbol(
            &optional_text(args, &["symbol"]).unwrap_or_else(|| "marker".to_string()),
        )
        .unwrap_or_else(|| "marker".to_string());
        if !is_supported_marker_symbol(&symbol) {
            return Err(RchCoreError::InvalidPayload(
                "Unsupported marker symbol".to_string(),
            ));
        }
        let mut category = normalize_marker_symbol(
            &optional_text(args, &["category"]).unwrap_or_else(|| "marker".to_string()),
        )
        .unwrap_or_default();
        if category.is_empty() {
            category.clone_from(&symbol);
        }
        let marker = MarkerRecord {
            local_id,
            object_destination_hash: object_destination_hash.clone(),
            origin_rch: normalize_hash(Some(&command.source.rns_identity)).unwrap_or_default(),
            marker_type,
            symbol,
            name: optional_text(args, &["name"]).unwrap_or_else(|| "Marker".to_string()),
            category,
            lat,
            lon,
            notes: optional_text(args, &["notes"]),
            created_ts_ms: now,
            updated_ts_ms: now,
        };
        self.markers.insert(object_destination_hash, marker.clone());
        Ok(marker)
    }

    fn patch_marker_position_from_args(
        &mut self,
        args: &Value,
    ) -> Result<MarkerRecord, RchCoreError> {
        let marker_hash = required_text(args, &["object_destination_hash"])?;
        let marker_hash = normalize_hash(Some(&marker_hash)).ok_or_else(|| {
            RchCoreError::InvalidPayload("object_destination_hash is required".to_string())
        })?;
        let lat = required_f64(args, "lat")?;
        let lon = required_f64(args, "lon")?;
        let marker = self
            .markers
            .get_mut(&marker_hash)
            .ok_or(RchCoreError::TopicNotFound)?;
        marker.lat = lat;
        marker.lon = lon;
        marker.updated_ts_ms = utc_now_ms();
        Ok(marker.clone())
    }

    fn patch_marker_from_args(&mut self, args: &Value) -> Result<MarkerRecord, RchCoreError> {
        let marker_hash = required_text(args, &["object_destination_hash"])?;
        let marker_hash = normalize_hash(Some(&marker_hash)).ok_or_else(|| {
            RchCoreError::InvalidPayload("object_destination_hash is required".to_string())
        })?;
        let marker = self
            .markers
            .get_mut(&marker_hash)
            .ok_or(RchCoreError::TopicNotFound)?;
        if let Some(name) = optional_text(args, &["name"]) {
            if name.trim().is_empty() {
                return Err(RchCoreError::InvalidPayload(
                    "Marker name is required".to_string(),
                ));
            }
            marker.name = name.trim().to_string();
        }
        marker.updated_ts_ms = utc_now_ms();
        Ok(marker.clone())
    }

    fn delete_marker_from_args(&mut self, args: &Value) -> Result<MarkerRecord, RchCoreError> {
        let marker_hash = required_text(args, &["object_destination_hash"])?;
        let marker_hash = normalize_hash(Some(&marker_hash)).ok_or_else(|| {
            RchCoreError::InvalidPayload("object_destination_hash is required".to_string())
        })?;
        self.markers
            .remove(&marker_hash)
            .ok_or(RchCoreError::TopicNotFound)
    }

    fn create_zone_from_args(&mut self, args: &Value) -> Result<ZoneRecord, RchCoreError> {
        let now = utc_now_ms();
        let zone = ZoneRecord {
            zone_id: Uuid::new_v4().simple().to_string(),
            name: optional_text(args, &["name"]).unwrap_or_else(|| "Zone".to_string()),
            points: required_zone_points(args)?,
            created_ts_ms: now,
            updated_ts_ms: now,
        };
        self.zones.insert(zone.zone_id.clone(), zone.clone());
        Ok(zone)
    }

    fn patch_zone_from_args(&mut self, args: &Value) -> Result<ZoneRecord, RchCoreError> {
        let zone_id = required_text(args, &["zone_id"])?;
        let zone = self
            .zones
            .get_mut(&zone_id)
            .ok_or(RchCoreError::TopicNotFound)?;
        if let Some(name) = optional_text(args, &["name"]) {
            zone.name = name;
        }
        if args
            .as_object()
            .is_some_and(|object| object.contains_key("points"))
        {
            zone.points = required_zone_points(args)?;
        }
        zone.updated_ts_ms = utc_now_ms();
        Ok(zone.clone())
    }

    fn delete_zone_from_args(&mut self, args: &Value) -> Result<ZoneRecord, RchCoreError> {
        let zone_id = required_text(args, &["zone_id"])?;
        self.zones
            .remove(&zone_id)
            .ok_or(RchCoreError::TopicNotFound)
    }

    fn upsert_mission_from_args(&mut self, args: &Value) -> Result<MissionRecord, RchCoreError> {
        let uid = optional_text(args, &["uid", "mission_id"])
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        if uid.trim().is_empty() {
            return Err(RchCoreError::InvalidPayload("uid is required".to_string()));
        }
        let now = utc_now_ms();
        let mut mission = self.missions.get(&uid).cloned().unwrap_or(MissionRecord {
            uid: uid.clone(),
            mission_name: "Mission".to_string(),
            description: String::new(),
            topic_id: None,
            path: None,
            classification: None,
            tool: None,
            keywords: Vec::new(),
            parent_uid: None,
            feeds: Vec::new(),
            password_hash: None,
            default_role: None,
            mission_priority: None,
            mission_status: "MISSION_ACTIVE".to_string(),
            owner_role: None,
            token: None,
            invite_only: false,
            expiration: None,
            mission_rde_role: None,
            created_ts_ms: now,
            updated_ts_ms: now,
        });

        self.apply_mission_identity_fields(args, &mut mission)?;
        Self::apply_mission_metadata_fields(args, &mut mission)?;
        self.ensure_parent_chain_acyclic(&uid, mission.parent_uid.as_deref())?;
        mission.updated_ts_ms = now;
        self.missions.insert(uid, mission.clone());
        Ok(mission)
    }

    fn apply_mission_identity_fields(
        &self,
        args: &Value,
        mission: &mut MissionRecord,
    ) -> Result<(), RchCoreError> {
        if let Some(name) = optional_text(args, &["mission_name", "name"]) {
            mission.mission_name = name;
        }
        if mission.mission_name.trim().is_empty() {
            return Err(RchCoreError::InvalidPayload(
                "mission_name is required".to_string(),
            ));
        }
        if let Some(description) = optional_text_or_empty(args, &["description"]) {
            mission.description = description;
        }
        if let Some(topic_id) = optional_text_or_empty(args, &["topic_id"]) {
            mission.topic_id = if topic_id.is_empty() {
                None
            } else {
                if !self.topics.contains_key(&topic_id) {
                    return Err(RchCoreError::TopicNotFound);
                }
                Some(topic_id)
            };
        }
        if let Some(path) = optional_text_or_empty(args, &["path"]) {
            mission.path = none_if_empty(path);
        }
        if let Some(classification) = optional_text_or_empty(args, &["classification"]) {
            mission.classification = none_if_empty(classification);
        }
        if let Some(tool) = optional_text_or_empty(args, &["tool"]) {
            mission.tool = none_if_empty(tool);
        }
        mission.parent_uid = Self::resolve_parent_uid(args, mission.parent_uid.clone());
        Ok(())
    }

    fn apply_mission_metadata_fields(
        args: &Value,
        mission: &mut MissionRecord,
    ) -> Result<(), RchCoreError> {
        if args
            .as_object()
            .is_some_and(|object| object.contains_key("keywords"))
        {
            mission.keywords = string_list(args.get("keywords"), "keywords")?;
        }
        if args
            .as_object()
            .is_some_and(|object| object.contains_key("feeds"))
        {
            mission.feeds = string_list(args.get("feeds"), "feeds")?;
        }
        apply_optional_role_fields(args, mission)?;
        apply_optional_mission_status_fields(args, mission)?;
        if let Some(token) = optional_text_or_empty(args, &["token"]) {
            mission.token = none_if_empty(token);
        }
        if let Some(invite_only) = optional_bool(args, "invite_only") {
            mission.invite_only = invite_only;
        }
        if let Some(expiration) = optional_text_or_empty(args, &["expiration"]) {
            mission.expiration = none_if_empty(expiration);
        }
        Ok(())
    }

    fn patch_mission_from_args(&mut self, args: &Value) -> Result<MissionRecord, RchCoreError> {
        let mission_uid = required_text(args, &["mission_uid", "uid"])?;
        if !self.missions.contains_key(&mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        let patch = args
            .get("patch")
            .filter(|value| value.is_object())
            .unwrap_or(args);
        let mut merged = patch.clone();
        merged["uid"] = Value::String(mission_uid);
        self.upsert_mission_from_args(&merged)
    }

    fn delete_mission_from_args(&mut self, args: &Value) -> Result<MissionRecord, RchCoreError> {
        let mission_uid = required_text(args, &["mission_uid", "uid"])?;
        let mission = self
            .missions
            .get_mut(&mission_uid)
            .ok_or(RchCoreError::MissionNotFound)?;
        mission.mission_status = "MISSION_DELETED".to_string();
        mission.updated_ts_ms = utc_now_ms();
        Ok(mission.clone())
    }

    fn set_mission_parent_from_args(
        &mut self,
        args: &Value,
    ) -> Result<MissionRecord, RchCoreError> {
        let mission_uid = required_text(args, &["mission_uid", "uid"])?;
        if !self.missions.contains_key(&mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        let parent_uid = optional_text_or_empty(args, &["parent_uid"]).and_then(none_if_empty);
        self.ensure_parent_chain_acyclic(&mission_uid, parent_uid.as_deref())?;
        let mission = self
            .missions
            .get_mut(&mission_uid)
            .ok_or(RchCoreError::MissionNotFound)?;
        mission.parent_uid = parent_uid;
        mission.updated_ts_ms = utc_now_ms();
        Ok(mission.clone())
    }

    fn link_mission_zone(
        &mut self,
        mission_uid: &str,
        zone_id: &str,
    ) -> Result<MissionRecord, RchCoreError> {
        let mission = self
            .missions
            .get_mut(mission_uid)
            .ok_or(RchCoreError::MissionNotFound)?;
        if !self.zones.contains_key(zone_id) {
            return Err(RchCoreError::InvalidPayload(format!(
                "Zone '{zone_id}' not found"
            )));
        }
        self.mission_zone_links
            .insert((mission_uid.to_string(), zone_id.to_string()));
        mission.updated_ts_ms = utc_now_ms();
        Ok(mission.clone())
    }

    fn unlink_mission_zone(
        &mut self,
        mission_uid: &str,
        zone_id: &str,
    ) -> Result<MissionRecord, RchCoreError> {
        let mission = self
            .missions
            .get_mut(mission_uid)
            .ok_or(RchCoreError::MissionNotFound)?;
        self.mission_zone_links
            .remove(&(mission_uid.to_string(), zone_id.to_string()));
        mission.updated_ts_ms = utc_now_ms();
        Ok(mission.clone())
    }

    fn link_mission_marker(
        &mut self,
        mission_uid: &str,
        marker_id: &str,
    ) -> Result<MissionRecord, RchCoreError> {
        self.ensure_marker_exists(marker_id)?;
        let mission = self
            .missions
            .get_mut(mission_uid)
            .ok_or(RchCoreError::MissionNotFound)?;
        self.mission_marker_links
            .insert((mission_uid.to_string(), marker_id.to_string()));
        mission.updated_ts_ms = utc_now_ms();
        Ok(mission.clone())
    }

    fn unlink_mission_marker(
        &mut self,
        mission_uid: &str,
        marker_id: &str,
    ) -> Result<MissionRecord, RchCoreError> {
        let mission = self
            .missions
            .get_mut(mission_uid)
            .ok_or(RchCoreError::MissionNotFound)?;
        self.mission_marker_links
            .remove(&(mission_uid.to_string(), marker_id.to_string()));
        mission.updated_ts_ms = utc_now_ms();
        Ok(mission.clone())
    }

    fn set_mission_rde(&mut self, mission_uid: &str, role: &str) -> Result<Value, RchCoreError> {
        let role = normalize_mission_role(role, "role")?;
        let mission = self
            .missions
            .get_mut(mission_uid)
            .ok_or(RchCoreError::MissionNotFound)?;
        let now = utc_now_ms();
        mission.mission_rde_role = Some(role.clone());
        mission.updated_ts_ms = now;
        Ok(json!({
            "mission_uid": mission_uid,
            "role": role,
            "updated_at": millis_to_rfc3339(now),
        }))
    }

    fn upsert_mission_change_from_args(
        &mut self,
        args: &Value,
    ) -> Result<MissionChangeRecord, RchCoreError> {
        let uid =
            optional_text(args, &["uid"]).unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let mission_uid = required_text(args, &["mission_uid", "mission_id"])?;
        if !self.missions.contains_key(&mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        let hashes = string_list(args.get("hashes"), "hashes")?;
        for marker_ref in &hashes {
            self.ensure_marker_exists(marker_ref)?;
        }
        let current = self.mission_changes.get(&uid);
        let change = MissionChangeRecord {
            uid: uid.clone(),
            mission_uid,
            name: optional_text_or_empty(args, &["name"])
                .or_else(|| current.and_then(|item| item.name.clone())),
            team_member_rns_identity: optional_text_or_empty(args, &["team_member_rns_identity"])
                .or_else(|| current.and_then(|item| item.team_member_rns_identity.clone())),
            timestamp_ms: optional_timestamp_ms(args, &["timestamp"])?
                .or_else(|| current.map(|item| item.timestamp_ms))
                .unwrap_or_else(utc_now_ms),
            notes: optional_text_or_empty(args, &["notes"])
                .or_else(|| current.and_then(|item| item.notes.clone())),
            change_type: if args
                .as_object()
                .is_some_and(|object| object.contains_key("change_type"))
            {
                normalize_mission_change_type(
                    args.get("change_type").and_then(value_as_str).as_deref(),
                )?
            } else {
                current.map_or_else(
                    || "ADD_CONTENT".to_string(),
                    |item| item.change_type.clone(),
                )
            },
            is_federated_change: optional_bool(args, "is_federated_change")
                .or_else(|| current.map(|item| item.is_federated_change))
                .unwrap_or(false),
            hashes,
            delta: args.get("delta").cloned().unwrap_or_else(|| json!({})),
        };
        if !change.delta.is_object() {
            return Err(RchCoreError::InvalidPayload(
                "delta must be an object".to_string(),
            ));
        }
        self.mission_changes.insert(uid, change.clone());
        Ok(change)
    }

    fn upsert_log_entry_from_args(
        &mut self,
        command: &MissionCommandEnvelope,
        args: &Value,
    ) -> Result<LogEntryRecord, RchCoreError> {
        let entry_uid = optional_text(args, &["entry_uid", "uid"])
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let mission_uid = self.resolve_log_mission_uid(args)?;
        let current = self.log_entries.get(&entry_uid);
        let content = optional_text(args, &["content"])
            .or_else(|| current.map(|entry| entry.content.clone()))
            .ok_or_else(|| RchCoreError::InvalidPayload("content is required".to_string()))?;
        let now = utc_now_ms();
        let content_hashes = if args.as_object().is_some_and(|object| {
            object.contains_key("content_hashes") || object.contains_key("contenthashes")
        }) {
            let raw = args
                .get("content_hashes")
                .or_else(|| args.get("contenthashes"));
            let hashes = string_list(raw, "content_hashes")?;
            for marker_ref in &hashes {
                self.ensure_marker_exists(marker_ref)?;
            }
            hashes
        } else {
            current.map_or_else(Vec::new, |entry| entry.content_hashes.clone())
        };
        let keywords = if args
            .as_object()
            .is_some_and(|object| object.contains_key("keywords"))
        {
            let mut keywords = string_list(args.get("keywords"), "keywords")?;
            append_mecp_keywords(&mut keywords, &content);
            keywords
        } else {
            let mut keywords = current.map_or_else(Vec::new, |entry| entry.keywords.clone());
            append_mecp_keywords(&mut keywords, &content);
            keywords
        };
        let entry = LogEntryRecord {
            entry_uid: entry_uid.clone(),
            mission_uid: mission_uid.clone(),
            callsign: Self::resolve_log_callsign(command, args, current),
            content,
            server_time_ms: optional_timestamp_ms(args, &["server_time", "servertime"])?
                .or_else(|| current.map(|entry| entry.server_time_ms))
                .unwrap_or(now),
            client_time: optional_text_or_empty(args, &["client_time", "clientTime", "clienttime"])
                .or_else(|| current.and_then(|entry| entry.client_time.clone())),
            content_hashes,
            keywords,
            created_ts_ms: current.map_or(now, |entry| entry.created_ts_ms),
            updated_ts_ms: now,
        };
        self.log_entries.insert(entry_uid, entry.clone());
        self.record_auto_log_change(&entry);
        Ok(entry)
    }

    fn resolve_log_mission_uid(&mut self, args: &Value) -> Result<String, RchCoreError> {
        if let Some(mission_uid) = optional_text(args, &["mission_uid", "mission_id"]) {
            if !self.missions.contains_key(&mission_uid) {
                return Err(RchCoreError::MissionNotFound);
            }
            return Ok(mission_uid);
        }
        self.ensure_default_log_mission();
        Ok("mission-default".to_string())
    }

    fn ensure_default_log_mission(&mut self) {
        if self.missions.contains_key("mission-default") {
            return;
        }
        let now = utc_now_ms();
        self.missions.insert(
            "mission-default".to_string(),
            MissionRecord {
                uid: "mission-default".to_string(),
                mission_name: "Mission Default".to_string(),
                description: "Synthetic mission used for missionless log submissions.".to_string(),
                topic_id: None,
                path: None,
                classification: None,
                tool: None,
                keywords: Vec::new(),
                parent_uid: None,
                feeds: Vec::new(),
                password_hash: None,
                default_role: None,
                mission_priority: None,
                mission_status: "MISSION_ACTIVE".to_string(),
                owner_role: None,
                token: None,
                invite_only: false,
                expiration: None,
                mission_rde_role: None,
                created_ts_ms: now,
                updated_ts_ms: now,
            },
        );
    }

    fn resolve_log_callsign(
        command: &MissionCommandEnvelope,
        args: &Value,
        current: Option<&LogEntryRecord>,
    ) -> Option<String> {
        optional_text_or_empty(args, &["callsign", "author_callsign"])
            .and_then(none_if_empty)
            .or_else(|| current.and_then(|entry| entry.callsign.clone()))
            .or_else(|| command.source.display_name.clone())
    }

    fn record_auto_log_change(&mut self, entry: &LogEntryRecord) {
        let change_uid = Uuid::new_v4().simple().to_string();
        let log_delta = json!({
            "op": "upsert",
            "entry_uid": entry.entry_uid,
            "mission_uid": entry.mission_uid,
            "callsign": entry.callsign,
            "content": entry.content,
            "server_time": millis_to_rfc3339(entry.server_time_ms),
            "client_time": entry.client_time,
            "keywords": entry.keywords,
            "mecp": mecp_log_entry_value(&entry.content),
            "content_hashes": entry.content_hashes,
        });
        self.mission_changes.insert(
            change_uid.clone(),
            MissionChangeRecord {
                uid: change_uid,
                mission_uid: entry.mission_uid.clone(),
                name: Some("mission.log_entry.upserted".to_string()),
                team_member_rns_identity: None,
                timestamp_ms: utc_now_ms(),
                notes: None,
                change_type: "ADD_CONTENT".to_string(),
                is_federated_change: false,
                hashes: entry.content_hashes.clone(),
                delta: json!({
                    "version": 1,
                    "contract_version": "mission.delta.v1",
                    "source_event_type": "mission.log_entry.upserted",
                    "emitted_at": utc_now_rfc3339(),
                    "logs": [log_delta],
                    "assets": [],
                    "tasks": [],
                    "checklists": [],
                }),
            },
        );
    }

    fn ensure_marker_exists(&self, marker_ref: &str) -> Result<(), RchCoreError> {
        let marker_ref = marker_ref.trim();
        if marker_ref.is_empty() {
            return Err(RchCoreError::InvalidPayload(
                "marker reference cannot be empty".to_string(),
            ));
        }
        if self.markers.contains_key(marker_ref)
            || self
                .markers
                .values()
                .any(|marker| marker.local_id == marker_ref)
        {
            Ok(())
        } else {
            Err(RchCoreError::InvalidPayload(format!(
                "Marker '{marker_ref}' not found"
            )))
        }
    }

    fn upsert_team_from_args(&mut self, args: &Value) -> Result<TeamRecord, RchCoreError> {
        let requested_uid = optional_text(args, &["uid"]);
        let canonical_team = canonical_team_from_team_args(args, requested_uid.as_deref());
        let uid = canonical_team
            .map(|team| team.0.to_string())
            .or(requested_uid)
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let now = utc_now_ms();
        let mut team = self.teams.get(&uid).cloned().unwrap_or(TeamRecord {
            uid: uid.clone(),
            mission_uid: None,
            mission_uids: Vec::new(),
            color: None,
            team_name: "Team".to_string(),
            team_description: String::new(),
            created_ts_ms: now,
            updated_ts_ms: now,
        });
        if team_refs_provided(args) {
            team.mission_uids = self.team_mission_uids_from_args(args)?;
            self.set_team_mission_links(&uid, &team.mission_uids);
            team.mission_uid = team.mission_uids.first().cloned();
        }
        if let Some((_, canonical_color)) = canonical_team {
            team.color = Some(canonical_color.to_string());
            team.team_name = canonical_color.to_string();
        } else {
            if let Some(color) = optional_text_or_empty(args, &["color"]) {
                team.color = none_if_empty(normalize_team_color(&color)?);
            }
            if let Some(name) = optional_text(args, &["team_name", "name"]) {
                team.team_name = name;
            }
        }
        if let Some(description) =
            optional_text_or_empty(args, &["team_description", "description"])
        {
            team.team_description = description;
        }
        team.updated_ts_ms = now;
        self.teams.insert(uid, team.clone());
        Ok(team)
    }

    fn upsert_eam_from_args(&mut self, args: &Value) -> Result<EamSnapshotRecord, RchCoreError> {
        reject_disallowed_eam_fields(args)?;
        let callsign = required_text(args, &["callsign"])?;
        let team_member_uid = required_text(args, &["team_member_uid"])?;
        let team_uid = required_text(args, &["team_uid"])?;
        self.ensure_eam_team_and_member(&team_uid, &team_member_uid, &callsign, args)?;
        let active_member_uid = self
            .eam_snapshots
            .values()
            .find(|eam| eam.team_member_uid == team_member_uid && eam.deleted_ts_ms.is_none())
            .map(|eam| eam.eam_uid.clone());
        let active_callsign_uid = self
            .eam_snapshots
            .values()
            .find(|eam| eam.callsign == callsign && eam.deleted_ts_ms.is_none())
            .map(|eam| eam.eam_uid.clone());
        if let Some(callsign_uid) = active_callsign_uid.as_deref() {
            if active_member_uid.as_deref() != Some(callsign_uid) {
                return Err(RchCoreError::InvalidPayload(format!(
                    "callsign '{callsign}' is already assigned to another status snapshot"
                )));
            }
        }
        let deleted_uids: HashSet<String> = self
            .eam_snapshots
            .values()
            .filter(|eam| {
                (eam.team_member_uid == team_member_uid || eam.callsign == callsign)
                    && eam.deleted_ts_ms.is_some()
            })
            .map(|eam| eam.eam_uid.clone())
            .collect();
        if deleted_uids.len() > 1 {
            return Err(RchCoreError::InvalidPayload(
                "eam_uid cannot be recreated because deleted subject and callsign snapshots refer to different records".to_string(),
            ));
        }
        let deleted_uid = deleted_uids.into_iter().next();
        let existing_uid = active_member_uid.or(active_callsign_uid).or(deleted_uid);
        let eam_uid = optional_text(args, &["eam_uid"])
            .or(existing_uid)
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let active_uid = self
            .eam_snapshots
            .values()
            .find(|eam| eam.team_member_uid == team_member_uid && eam.deleted_ts_ms.is_none())
            .map(|eam| eam.eam_uid.clone());
        if let Some(active_uid) = active_uid {
            if optional_text(args, &["eam_uid"]).is_some_and(|provided| provided != active_uid) {
                return Err(RchCoreError::InvalidPayload(format!(
                    "eam_uid '{eam_uid}' does not match the existing snapshot for team_member_uid '{team_member_uid}'"
                )));
            }
        }
        let now = utc_now_ms();
        let statuses = EamStatuses::from_args(args)?;
        let confidence = optional_f64(args, "confidence")?;
        if confidence.is_some_and(|value| !(0.0..=1.0).contains(&value)) {
            return Err(RchCoreError::InvalidPayload(
                "confidence must be between 0 and 1".to_string(),
            ));
        }
        let ttl_seconds = optional_i64(args, "ttl_seconds")?;
        if ttl_seconds.is_some_and(|value| value < 0) {
            return Err(RchCoreError::InvalidPayload(
                "ttl_seconds must be greater than or equal to 0".to_string(),
            ));
        }
        let eam = EamSnapshotRecord {
            eam_uid: eam_uid.clone(),
            callsign,
            group_name: optional_text_or_empty(args, &["group_name"])
                .and_then(none_if_empty)
                .or_else(|| self.eam_group_name_for_team(&team_uid)),
            team_member_uid,
            team_uid,
            reported_by: optional_text_or_empty(args, &["reported_by"]).and_then(none_if_empty),
            reported_ts_ms: optional_timestamp_ms(args, &["reported_at"])?.unwrap_or(now),
            overall_status: aggregate_eam_status(&statuses),
            security_status: statuses.security,
            capability_status: statuses.capability,
            preparedness_status: statuses.preparedness,
            medical_status: statuses.medical,
            mobility_status: statuses.mobility,
            comms_status: statuses.comms,
            notes: optional_text_or_empty(args, &["notes"]).and_then(none_if_empty),
            confidence,
            ttl_seconds,
            source: args
                .get("source")
                .filter(|value| value.is_object())
                .cloned(),
            updated_ts_ms: now,
            deleted_ts_ms: None,
        };
        self.eam_snapshots.insert(eam_uid, eam.clone());
        Ok(eam)
    }

    fn ensure_eam_team_and_member(
        &mut self,
        team_uid: &str,
        team_member_uid: &str,
        callsign: &str,
        args: &Value,
    ) -> Result<(), RchCoreError> {
        let canonical_color = canonical_team_color_for_uid(team_uid);
        if let Some(group_name) = optional_text(args, &["group_name"]) {
            if let Some(canonical_color) = canonical_color {
                if normalize_team_color(&group_name).ok().as_deref() != Some(canonical_color) {
                    return Err(RchCoreError::InvalidPayload(format!(
                        "group_name '{group_name}' does not match canonical team_uid '{team_uid}'"
                    )));
                }
            }
        }

        if !self.teams.contains_key(team_uid) {
            let canonical_color = canonical_color.ok_or_else(|| {
                RchCoreError::InvalidPayload(format!(
                    "team_uid '{team_uid}' does not map to a team"
                ))
            })?;
            let now = utc_now_ms();
            self.teams.insert(
                team_uid.to_string(),
                TeamRecord {
                    uid: team_uid.to_string(),
                    mission_uid: None,
                    mission_uids: Vec::new(),
                    color: Some(canonical_color.to_string()),
                    team_name: canonical_color.to_string(),
                    team_description: String::new(),
                    created_ts_ms: now,
                    updated_ts_ms: now,
                },
            );
        } else if let Some(canonical_color) = canonical_color {
            let team = self
                .teams
                .get_mut(team_uid)
                .ok_or(RchCoreError::TeamNotFound)?;
            team.color = Some(canonical_color.to_string());
            team.team_name = canonical_color.to_string();
            team.updated_ts_ms = utc_now_ms();
        }
        if let Some(member) = self.team_members.get(team_member_uid) {
            if member.team_uid.as_deref() != Some(team_uid) {
                return Err(RchCoreError::InvalidPayload(format!(
                    "team_member_uid '{team_member_uid}' does not belong to team_uid '{team_uid}'"
                )));
            }
        } else {
            let identity = args
                .get("source")
                .and_then(|source| optional_text(source, &["rns_identity"]))
                .ok_or_else(|| {
                    RchCoreError::InvalidPayload(format!(
                        "team_member_uid '{team_member_uid}' is missing and source.rns_identity is required to provision it"
                    ))
                })?;
            let now = utc_now_ms();
            self.team_members.insert(
                team_member_uid.to_string(),
                TeamMemberRecord {
                    uid: team_member_uid.to_string(),
                    team_uid: Some(team_uid.to_string()),
                    rns_identity: identity,
                    display_name: optional_text(args, &["reported_by"])
                        .unwrap_or_else(|| callsign.to_string()),
                    icon: None,
                    role: None,
                    callsign: Some(callsign.to_string()),
                    freq: None,
                    email: None,
                    phone: None,
                    modulation: None,
                    availability: None,
                    certifications: Vec::new(),
                    last_active: None,
                    client_identities: Vec::new(),
                    created_ts_ms: now,
                    updated_ts_ms: now,
                },
            );
        }
        Ok(())
    }

    fn active_eam_by_callsign(&self, callsign: &str) -> Result<&EamSnapshotRecord, RchCoreError> {
        self.eam_snapshots
            .values()
            .find(|eam| {
                eam.callsign == callsign && eam.deleted_ts_ms.is_none() && !eam_is_expired(eam)
            })
            .ok_or(RchCoreError::EamNotFound)
    }

    fn latest_eam_by_member(&self, member: &str) -> Result<&EamSnapshotRecord, RchCoreError> {
        self.eam_snapshots
            .values()
            .filter(|eam| {
                eam.team_member_uid == member && eam.deleted_ts_ms.is_none() && !eam_is_expired(eam)
            })
            .max_by(|left, right| left.reported_ts_ms.cmp(&right.reported_ts_ms))
            .ok_or(RchCoreError::EamNotFound)
    }

    fn delete_eam_by_callsign(
        &mut self,
        callsign: &str,
    ) -> Result<EamSnapshotRecord, RchCoreError> {
        let eam_uid = self.active_eam_by_callsign(callsign)?.eam_uid.clone();
        let eam = self
            .eam_snapshots
            .get_mut(&eam_uid)
            .ok_or(RchCoreError::EamNotFound)?;
        eam.deleted_ts_ms = Some(utc_now_ms());
        Ok(eam.clone())
    }

    fn delete_team_from_args(&mut self, args: &Value) -> Result<TeamRecord, RchCoreError> {
        let team_uid = required_text(args, &["team_uid", "uid"])?;
        let team = self
            .teams
            .remove(&team_uid)
            .ok_or(RchCoreError::TeamNotFound)?;
        for member in self.team_members.values_mut() {
            if member.team_uid.as_deref() == Some(&team_uid) {
                member.team_uid = None;
                member.updated_ts_ms = utc_now_ms();
            }
        }
        self.mission_team_links
            .retain(|(_, linked_team_uid)| linked_team_uid != &team_uid);
        Ok(team)
    }

    fn link_team_mission_from_args(&mut self, args: &Value) -> Result<TeamRecord, RchCoreError> {
        let team_uid = required_text(args, &["team_uid", "uid"])?;
        let mission_uid = required_text(args, &["mission_uid", "mission_id"])?;
        if !self.missions.contains_key(&mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        let mut mission_uids = self.team_mission_ids(&team_uid);
        if !mission_uids.contains(&mission_uid) {
            mission_uids.push(mission_uid);
        }
        mission_uids = dedupe_non_empty(mission_uids);
        self.set_team_mission_links(&team_uid, &mission_uids);
        self.update_team_mission_refs(&team_uid, mission_uids)
    }

    fn unlink_team_mission_from_args(&mut self, args: &Value) -> Result<TeamRecord, RchCoreError> {
        let team_uid = required_text(args, &["team_uid", "uid"])?;
        let mission_uid = required_text(args, &["mission_uid", "mission_id"])?;
        let mission_uids: Vec<_> = self
            .team_mission_ids(&team_uid)
            .into_iter()
            .filter(|uid| uid != &mission_uid)
            .collect();
        self.set_team_mission_links(&team_uid, &mission_uids);
        self.update_team_mission_refs(&team_uid, mission_uids)
    }

    fn team_mission_uids_from_args(&self, args: &Value) -> Result<Vec<String>, RchCoreError> {
        let mut mission_uids = if args
            .as_object()
            .is_some_and(|object| object.contains_key("mission_uids"))
        {
            string_list(args.get("mission_uids"), "mission_uids")?
        } else {
            Vec::new()
        };
        if let Some(mission_uid) = optional_text(args, &["mission_uid", "mission_id"]) {
            mission_uids.push(mission_uid);
        }
        let mission_uids = dedupe_non_empty(mission_uids);
        for mission_uid in &mission_uids {
            if !self.missions.contains_key(mission_uid) {
                return Err(RchCoreError::MissionNotFound);
            }
        }
        Ok(mission_uids)
    }

    fn set_team_mission_links(&mut self, team_uid: &str, mission_uids: &[String]) {
        self.mission_team_links
            .retain(|(_, linked_team_uid)| linked_team_uid != team_uid);
        self.mission_team_links.extend(
            mission_uids
                .iter()
                .map(|mission_uid| (mission_uid.clone(), team_uid.to_string())),
        );
    }

    fn update_team_mission_refs(
        &mut self,
        team_uid: &str,
        mission_uids: Vec<String>,
    ) -> Result<TeamRecord, RchCoreError> {
        let team = self
            .teams
            .get_mut(team_uid)
            .ok_or(RchCoreError::TeamNotFound)?;
        team.mission_uid = mission_uids.first().cloned();
        team.mission_uids = mission_uids;
        team.updated_ts_ms = utc_now_ms();
        Ok(team.clone())
    }

    fn upsert_team_member_from_args(
        &mut self,
        args: &Value,
    ) -> Result<TeamMemberRecord, RchCoreError> {
        let uid =
            optional_text(args, &["uid"]).unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let identity = required_text(args, &["rns_identity", "team_member_rns_identity"])?;
        let now = utc_now_ms();
        let mut member = self
            .team_members
            .get(&uid)
            .cloned()
            .unwrap_or(TeamMemberRecord {
                uid: uid.clone(),
                team_uid: None,
                rns_identity: identity.clone(),
                display_name: identity.clone(),
                icon: None,
                role: None,
                callsign: None,
                freq: None,
                email: None,
                phone: None,
                modulation: None,
                availability: None,
                certifications: Vec::new(),
                last_active: None,
                client_identities: Vec::new(),
                created_ts_ms: now,
                updated_ts_ms: now,
            });
        member.rns_identity = identity;
        self.apply_team_member_fields(args, &mut member)?;
        member.client_identities = self.team_member_client_ids(&uid);
        member.updated_ts_ms = now;
        self.team_members.insert(uid, member.clone());
        Ok(member)
    }

    fn apply_team_member_fields(
        &self,
        args: &Value,
        member: &mut TeamMemberRecord,
    ) -> Result<(), RchCoreError> {
        if args
            .as_object()
            .is_some_and(|object| object.contains_key("team_uid"))
        {
            let team_uid = optional_text_or_empty(args, &["team_uid"]).and_then(none_if_empty);
            if let Some(team_uid) = &team_uid {
                if !self.teams.contains_key(team_uid) {
                    return Err(RchCoreError::TeamNotFound);
                }
            }
            member.team_uid = team_uid;
        }
        if let Some(display_name) = optional_text(args, &["display_name", "callsign"]) {
            member.display_name = display_name;
        }
        if let Some(icon) = optional_text_or_empty(args, &["icon"]) {
            member.icon = none_if_empty(icon);
        }
        if let Some(role) = optional_text_or_empty(args, &["role"]) {
            member.role = none_if_empty(normalize_team_role(&role)?);
        }
        if let Some(callsign) = optional_text_or_empty(args, &["callsign"]) {
            member.callsign = none_if_empty(callsign);
        }
        member.freq = optional_f64(args, "freq")?.or(member.freq);
        apply_optional_member_contact_fields(args, member)?;
        Ok(())
    }

    fn delete_team_member_from_args(
        &mut self,
        args: &Value,
    ) -> Result<TeamMemberRecord, RchCoreError> {
        let uid = required_text(args, &["team_member_uid", "uid"])?;
        let member = self
            .team_members
            .remove(&uid)
            .ok_or(RchCoreError::TeamMemberNotFound)?;
        for asset in self.assets.values_mut() {
            if asset.team_member_uid.as_deref() == Some(&uid) {
                asset.team_member_uid = None;
                asset.updated_ts_ms = utc_now_ms();
            }
        }
        self.team_member_client_links
            .retain(|(member_uid, _)| member_uid != &uid);
        self.team_member_skills
            .retain(|_, skill| skill.team_member_rns_identity != member.rns_identity);
        Ok(member)
    }

    fn link_team_member_client_from_args(
        &mut self,
        args: &Value,
    ) -> Result<TeamMemberRecord, RchCoreError> {
        let uid = required_text(args, &["team_member_uid", "uid"])?;
        let identity = normalize_required_identity(
            optional_text(args, &["client_identity"]).as_deref(),
            "client_identity",
        )?;
        if !self.team_members.contains_key(&uid) {
            return Err(RchCoreError::TeamMemberNotFound);
        }
        self.team_member_client_links
            .insert((uid.clone(), identity.clone()));
        self.refresh_team_member_clients(&uid)
    }

    fn unlink_team_member_client_from_args(
        &mut self,
        args: &Value,
    ) -> Result<TeamMemberRecord, RchCoreError> {
        let uid = required_text(args, &["team_member_uid", "uid"])?;
        let identity = normalize_required_identity(
            optional_text(args, &["client_identity"]).as_deref(),
            "client_identity",
        )?;
        self.team_member_client_links
            .remove(&(uid.clone(), identity));
        self.refresh_team_member_clients(&uid)
    }

    fn refresh_team_member_clients(&mut self, uid: &str) -> Result<TeamMemberRecord, RchCoreError> {
        let client_identities = self.team_member_client_ids(uid);
        let member = self
            .team_members
            .get_mut(uid)
            .ok_or(RchCoreError::TeamMemberNotFound)?;
        member.client_identities = client_identities;
        member.updated_ts_ms = utc_now_ms();
        Ok(member.clone())
    }

    fn team_member_client_ids(&self, uid: &str) -> Vec<String> {
        let mut identities: Vec<_> = self
            .team_member_client_links
            .iter()
            .filter(|(member_uid, _)| member_uid == uid)
            .map(|(_, identity)| identity.clone())
            .collect();
        identities.sort();
        identities
    }

    fn upsert_asset_from_args(&mut self, args: &Value) -> Result<AssetRecord, RchCoreError> {
        let asset_uid = optional_text(args, &["asset_uid"])
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let now = utc_now_ms();
        let mut asset = self.assets.get(&asset_uid).cloned().unwrap_or(AssetRecord {
            asset_uid: asset_uid.clone(),
            team_member_uid: None,
            name: "Asset".to_string(),
            asset_type: "generic".to_string(),
            serial_number: None,
            status: "AVAILABLE".to_string(),
            location: None,
            notes: None,
            created_ts_ms: now,
            updated_ts_ms: now,
        });
        if let Some(team_member_uid) = optional_text_or_empty(args, &["team_member_uid"]) {
            let team_member_uid = none_if_empty(team_member_uid);
            if let Some(team_member_uid) = &team_member_uid {
                if !self.team_members.contains_key(team_member_uid) {
                    return Err(RchCoreError::TeamMemberNotFound);
                }
            }
            asset.team_member_uid = team_member_uid;
        }
        if let Some(name) = optional_text(args, &["name"]) {
            asset.name = name;
        }
        if let Some(asset_type) = optional_text(args, &["asset_type"]) {
            asset.asset_type = asset_type;
        }
        if let Some(serial_number) = optional_text_or_empty(args, &["serial_number"]) {
            asset.serial_number = none_if_empty(serial_number);
        }
        if let Some(status) = optional_text(args, &["status"]) {
            asset.status = normalize_asset_status(&status)?;
        }
        if let Some(location) = optional_text_or_empty(args, &["location"]) {
            asset.location = none_if_empty(location);
        }
        if let Some(notes) = optional_text_or_empty(args, &["notes"]) {
            asset.notes = none_if_empty(notes);
        }
        asset.updated_ts_ms = now;
        self.assets.insert(asset_uid, asset.clone());
        Ok(asset)
    }

    fn delete_asset(&mut self, asset_uid: &str) -> Result<AssetRecord, RchCoreError> {
        self.assets
            .remove(asset_uid)
            .ok_or(RchCoreError::AssetNotFound)
            .inspect(|_| {
                self.assignment_asset_links
                    .retain(|(_, linked_asset_uid)| linked_asset_uid != asset_uid);
                for assignment in self.assignments.values_mut() {
                    assignment.assets.retain(|item| item != asset_uid);
                }
            })
    }

    fn upsert_skill_from_args(&mut self, args: &Value) -> SkillRecord {
        let skill_uid = optional_text(args, &["skill_uid"])
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let now = utc_now_ms();
        let mut skill = self.skills.get(&skill_uid).cloned().unwrap_or(SkillRecord {
            skill_uid: skill_uid.clone(),
            name: "Skill".to_string(),
            category: None,
            description: None,
            proficiency_scale: None,
            created_ts_ms: now,
            updated_ts_ms: now,
        });
        if let Some(name) = optional_text(args, &["name"]) {
            skill.name = name;
        }
        if let Some(category) = optional_text_or_empty(args, &["category"]) {
            skill.category = none_if_empty(category);
        }
        if let Some(description) = optional_text_or_empty(args, &["description"]) {
            skill.description = none_if_empty(description);
        }
        if let Some(scale) = optional_text_or_empty(args, &["proficiency_scale"]) {
            skill.proficiency_scale = none_if_empty(scale);
        }
        skill.updated_ts_ms = now;
        self.skills.insert(skill_uid, skill.clone());
        skill
    }

    fn upsert_team_member_skill_from_args(
        &mut self,
        args: &Value,
    ) -> Result<TeamMemberSkillRecord, RchCoreError> {
        let identity = required_text(args, &["team_member_rns_identity"])?;
        if !self
            .team_members
            .values()
            .any(|member| member.rns_identity == identity)
        {
            return Err(RchCoreError::TeamMemberNotFound);
        }
        let skill_uid = required_text(args, &["skill_uid"])?;
        if !self.skills.contains_key(&skill_uid) {
            return Err(RchCoreError::SkillNotFound);
        }
        let key = format!("{identity}:{skill_uid}");
        let current = self.team_member_skills.get(&key);
        let level = optional_i64(args, "level")?
            .or_else(|| current.map(|record| record.level))
            .unwrap_or(0);
        let record = TeamMemberSkillRecord {
            uid: optional_text(args, &["uid"])
                .or_else(|| current.map(|record| record.uid.clone()))
                .unwrap_or_else(|| Uuid::new_v4().simple().to_string()),
            team_member_rns_identity: identity,
            skill_uid,
            level: normalize_skill_level(level, "level")?,
            validated_by: optional_text_or_empty(args, &["validated_by"])
                .and_then(none_if_empty)
                .or_else(|| current.and_then(|record| record.validated_by.clone())),
            validated_at: optional_text_or_empty(args, &["validated_at"])
                .and_then(none_if_empty)
                .or_else(|| current.and_then(|record| record.validated_at.clone())),
            expires_at: optional_text_or_empty(args, &["expires_at"])
                .and_then(none_if_empty)
                .or_else(|| current.and_then(|record| record.expires_at.clone())),
        };
        self.team_member_skills.insert(key, record.clone());
        Ok(record)
    }

    fn upsert_task_skill_requirement_from_args(
        &mut self,
        args: &Value,
    ) -> Result<TaskSkillRequirementRecord, RchCoreError> {
        let task_uid = required_text(args, &["task_uid"])?;
        let skill_uid = required_text(args, &["skill_uid"])?;
        if !self.checklist_tasks.contains_key(&task_uid) {
            return Err(RchCoreError::InvalidPayload(format!(
                "Checklist task '{task_uid}' not found"
            )));
        }
        if !self.skills.contains_key(&skill_uid) {
            return Err(RchCoreError::SkillNotFound);
        }
        let key = format!("{task_uid}:{skill_uid}");
        let current = self.task_skill_requirements.get(&key);
        let minimum_level = optional_i64(args, "minimum_level")?
            .or_else(|| current.map(|record| record.minimum_level))
            .unwrap_or(0);
        let record = TaskSkillRequirementRecord {
            uid: optional_text(args, &["uid"])
                .or_else(|| current.map(|record| record.uid.clone()))
                .unwrap_or_else(|| Uuid::new_v4().simple().to_string()),
            task_uid,
            skill_uid,
            minimum_level: normalize_skill_level(minimum_level, "minimum_level")?,
            is_mandatory: optional_bool(args, "is_mandatory")
                .or_else(|| current.map(|record| record.is_mandatory))
                .unwrap_or(true),
        };
        self.task_skill_requirements.insert(key, record.clone());
        Ok(record)
    }

    fn upsert_assignment_from_args(
        &mut self,
        args: &Value,
    ) -> Result<AssignmentRecord, RchCoreError> {
        let assignment_uid = optional_text(args, &["assignment_uid"])
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let current = self.assignments.get(&assignment_uid);
        let mission_uid = optional_text(args, &["mission_uid", "mission_id"])
            .or_else(|| current.map(|record| record.mission_uid.clone()))
            .ok_or_else(|| {
                RchCoreError::InvalidPayload(
                    "mission_uid, task_uid and team_member_rns_identity are required".to_string(),
                )
            })?;
        let task_uid = optional_text(args, &["task_uid"])
            .or_else(|| current.map(|record| record.task_uid.clone()))
            .ok_or_else(|| {
                RchCoreError::InvalidPayload(
                    "mission_uid, task_uid and team_member_rns_identity are required".to_string(),
                )
            })?;
        let member = optional_text(args, &["team_member_rns_identity"])
            .or_else(|| current.map(|record| record.team_member_rns_identity.clone()))
            .ok_or_else(|| {
                RchCoreError::InvalidPayload(
                    "mission_uid, task_uid and team_member_rns_identity are required".to_string(),
                )
            })?;
        if !self.missions.contains_key(&mission_uid) {
            return Err(RchCoreError::MissionNotFound);
        }
        if !self.checklist_tasks.contains_key(&task_uid) {
            return Err(RchCoreError::InvalidPayload(format!(
                "Checklist task '{task_uid}' not found"
            )));
        }
        self.ensure_team_member_identity_exists(&member)?;
        let assets = if args
            .as_object()
            .is_some_and(|object| object.contains_key("assets"))
        {
            self.validate_asset_refs(string_list(args.get("assets"), "assets")?)?
        } else {
            current.map_or_else(Vec::new, |record| record.assets.clone())
        };
        let now = utc_now_ms();
        let assigned_ts_ms = optional_timestamp_ms(args, &["assigned_at"])?
            .or_else(|| current.map(|record| record.assigned_ts_ms))
            .unwrap_or(now);
        let status = optional_text(args, &["status"])
            .map(|value| normalize_task_status(&value))
            .transpose()?
            .or_else(|| current.map(|record| record.status.clone()))
            .unwrap_or_else(|| "PENDING".to_string());
        let assignment = AssignmentRecord {
            assignment_uid: assignment_uid.clone(),
            mission_uid,
            task_uid,
            team_member_rns_identity: member,
            assigned_by: optional_text_or_empty(args, &["assigned_by"])
                .and_then(none_if_empty)
                .or_else(|| current.and_then(|record| record.assigned_by.clone())),
            assigned_ts_ms,
            due_dtg: optional_text_or_empty(args, &["due_dtg"])
                .and_then(none_if_empty)
                .or_else(|| current.and_then(|record| record.due_dtg.clone())),
            status,
            notes: optional_text_or_empty(args, &["notes"])
                .and_then(none_if_empty)
                .or_else(|| current.and_then(|record| record.notes.clone())),
            assets,
        };
        self.assignments
            .insert(assignment_uid.clone(), assignment.clone());
        self.set_assignment_asset_links(&assignment_uid, &assignment.assets);
        Ok(assignment)
    }

    fn set_assignment_assets(
        &mut self,
        assignment_uid: &str,
        assets: Vec<String>,
    ) -> Result<AssignmentRecord, RchCoreError> {
        let assets = self.validate_asset_refs(assets)?;
        let assignment = self
            .assignments
            .get_mut(assignment_uid)
            .ok_or(RchCoreError::AssignmentNotFound)?;
        assignment.assets.clone_from(&assets);
        let assignment = assignment.clone();
        self.set_assignment_asset_links(assignment_uid, &assets);
        Ok(assignment)
    }

    fn link_assignment_asset(
        &mut self,
        assignment_uid: &str,
        asset_uid: &str,
    ) -> Result<AssignmentRecord, RchCoreError> {
        let asset_uid = required_non_empty(asset_uid, "asset_uid")?;
        self.validate_asset_refs(vec![asset_uid.clone()])?;
        let assignment = self
            .assignments
            .get_mut(assignment_uid)
            .ok_or(RchCoreError::AssignmentNotFound)?;
        if !assignment.assets.contains(&asset_uid) {
            assignment.assets.push(asset_uid);
        }
        let assignment = assignment.clone();
        self.set_assignment_asset_links(assignment_uid, &assignment.assets);
        Ok(assignment)
    }

    fn unlink_assignment_asset(
        &mut self,
        assignment_uid: &str,
        asset_uid: &str,
    ) -> Result<AssignmentRecord, RchCoreError> {
        let asset_uid = required_non_empty(asset_uid, "asset_uid")?;
        let assignment = self
            .assignments
            .get_mut(assignment_uid)
            .ok_or(RchCoreError::AssignmentNotFound)?;
        assignment.assets.retain(|item| item != &asset_uid);
        let assignment = assignment.clone();
        self.set_assignment_asset_links(assignment_uid, &assignment.assets);
        Ok(assignment)
    }

    fn set_assignment_asset_links(&mut self, assignment_uid: &str, assets: &[String]) {
        self.assignment_asset_links
            .retain(|(linked_assignment_uid, _)| linked_assignment_uid != assignment_uid);
        self.assignment_asset_links.extend(
            assets
                .iter()
                .map(|asset_uid| (assignment_uid.to_string(), asset_uid.clone())),
        );
    }

    fn validate_asset_refs(&self, assets: Vec<String>) -> Result<Vec<String>, RchCoreError> {
        let assets = dedupe_non_empty(assets);
        for asset_uid in &assets {
            if !self.assets.contains_key(asset_uid) {
                return Err(RchCoreError::AssetNotFound);
            }
        }
        Ok(assets)
    }

    fn ensure_team_member_identity_exists(&self, identity: &str) -> Result<(), RchCoreError> {
        if self
            .team_members
            .values()
            .any(|member| member.rns_identity == identity)
        {
            Ok(())
        } else {
            Err(RchCoreError::TeamMemberNotFound)
        }
    }

    fn create_checklist_template(
        &mut self,
        template: &Value,
        source_template_uid: Option<String>,
    ) -> Result<ChecklistTemplateRecord, RchCoreError> {
        let now = utc_now_ms();
        let uid = optional_text(template, &["uid"])
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let record = ChecklistTemplateRecord {
            uid: uid.clone(),
            template_name: optional_text(template, &["template_name", "name"])
                .unwrap_or_else(|| "Template".to_string()),
            description: optional_text_or_empty(template, &["description"]).unwrap_or_default(),
            created_by_team_member_rns_identity: optional_text(
                template,
                &["created_by_team_member_rns_identity", "created_by"],
            )
            .unwrap_or_else(|| "unknown".to_string()),
            source_template_uid: source_template_uid.or_else(|| {
                optional_text_or_empty(template, &["source_template_uid"]).and_then(none_if_empty)
            }),
            server_only: optional_bool(template, "server_only").unwrap_or(true),
            created_ts_ms: optional_timestamp_ms(template, &["created_at"])?.unwrap_or(now),
            updated_ts_ms: optional_timestamp_ms(template, &["updated_at"])?.unwrap_or(now),
        };
        let columns =
            defaulted_checklist_columns(checklist_columns_from_args(template.get("columns"))?);
        validate_checklist_columns(&columns)?;
        self.checklist_templates.insert(uid.clone(), record.clone());
        self.replace_template_columns(&uid, columns, now);
        Ok(record)
    }

    fn update_checklist_template(
        &mut self,
        template_uid: &str,
        patch: &Value,
    ) -> Result<ChecklistTemplateRecord, RchCoreError> {
        if !self.checklist_templates.contains_key(template_uid) {
            return Err(RchCoreError::InvalidPayload(format!(
                "Checklist template '{template_uid}' not found"
            )));
        }
        if patch
            .as_object()
            .is_some_and(|object| object.contains_key("columns"))
        {
            let columns = checklist_columns_from_args(patch.get("columns"))?;
            validate_checklist_columns(&columns)?;
            self.replace_template_columns(template_uid, columns, utc_now_ms());
        }
        let template = self
            .checklist_templates
            .get_mut(template_uid)
            .ok_or_else(|| RchCoreError::InvalidPayload("template disappeared".to_string()))?;
        if let Some(name) = optional_text(patch, &["template_name", "name"]) {
            template.template_name = name;
        }
        if let Some(description) = optional_text_or_empty(patch, &["description"]) {
            template.description = description;
        }
        if let Some(source_uid) = optional_text_or_empty(patch, &["source_template_uid"]) {
            template.source_template_uid = none_if_empty(source_uid);
        }
        template.updated_ts_ms = utc_now_ms();
        Ok(template.clone())
    }

    fn clone_checklist_template(
        &mut self,
        command: &MissionCommandEnvelope,
        source_uid: &str,
        name: &str,
    ) -> Result<ChecklistTemplateRecord, RchCoreError> {
        let source = self
            .checklist_templates
            .get(source_uid)
            .ok_or_else(|| {
                RchCoreError::InvalidPayload(format!("Checklist template '{source_uid}' not found"))
            })?
            .clone();
        let columns = self.template_column_values(source_uid);
        let payload = json!({
            "template_name": name,
            "description": optional_text_or_empty(&command.args, &["description"]).unwrap_or(source.description),
            "created_by_team_member_rns_identity": command.source.rns_identity,
            "source_template_uid": source_uid,
            "columns": columns,
        });
        self.create_checklist_template(&payload, Some(source_uid.to_string()))
    }

    fn delete_checklist_template(&mut self, template_uid: &str) -> Result<Value, RchCoreError> {
        let template = self
            .checklist_templates
            .remove(template_uid)
            .ok_or_else(|| {
                RchCoreError::InvalidPayload(format!(
                    "Checklist template '{template_uid}' not found"
                ))
            })?;
        let payload = self.checklist_template_value(&template);
        self.checklist_columns
            .retain(|_, column| column.template_uid.as_deref() != Some(template_uid));
        Ok(payload)
    }

    fn replace_template_columns(&mut self, template_uid: &str, columns: Vec<Value>, now: i64) {
        self.checklist_columns
            .retain(|_, column| column.template_uid.as_deref() != Some(template_uid));
        for (index, column) in columns.into_iter().enumerate() {
            let display_order = column
                .get("display_order")
                .and_then(value_as_i64)
                .unwrap_or_else(|| i64::try_from(index + 1).unwrap_or(1));
            let record = ChecklistColumnRecord {
                column_uid: Uuid::new_v4().simple().to_string(),
                checklist_uid: None,
                template_uid: Some(template_uid.to_string()),
                column_name: optional_text(&column, &["column_name", "name"])
                    .unwrap_or_else(|| "Column".to_string()),
                display_order,
                column_type: optional_text(&column, &["column_type"])
                    .and_then(|value| normalize_checklist_column_type(&value).ok())
                    .unwrap_or_else(|| "SHORT_STRING".to_string()),
                column_editable: optional_bool(&column, "column_editable").unwrap_or(true),
                background_color: optional_text_or_empty(&column, &["background_color"])
                    .and_then(none_if_empty),
                text_color: optional_text_or_empty(&column, &["text_color"])
                    .and_then(none_if_empty),
                is_removable: optional_bool(&column, "is_removable").unwrap_or(true),
                system_key: optional_text_or_empty(&column, &["system_key"])
                    .and_then(none_if_empty),
                created_ts_ms: now,
                updated_ts_ms: now,
            };
            self.checklist_columns
                .insert(record.column_uid.clone(), record);
        }
    }

    fn template_column_values(&self, template_uid: &str) -> Vec<Value> {
        let mut columns: Vec<_> = self
            .checklist_columns
            .values()
            .filter(|column| column.template_uid.as_deref() == Some(template_uid))
            .cloned()
            .collect();
        columns.sort_by_key(|column| column.display_order);
        columns
            .into_iter()
            .map(|column| checklist_column_record_value(&column))
            .collect()
    }

    fn create_checklist_from_args(
        &mut self,
        command: &MissionCommandEnvelope,
        mode: &str,
        default_sync_state: &str,
    ) -> Result<ChecklistRecord, RchCoreError> {
        let name = required_text(&command.args, &["name"])?;
        let mut columns = checklist_columns_from_args(command.args.get("columns"))?;
        let template_uid = optional_text(&command.args, &["template_uid"]);
        let template_name = if let Some(template_uid) = &template_uid {
            let template = self.checklist_templates.get(template_uid).ok_or_else(|| {
                RchCoreError::InvalidPayload(format!(
                    "Checklist template '{template_uid}' not found"
                ))
            })?;
            if columns.is_empty() {
                columns = self.template_column_values(template_uid);
            }
            Some(template.template_name.clone())
        } else {
            None
        };
        if mode == "ONLINE" && columns.is_empty() && template_uid.is_none() {
            return Err(RchCoreError::InvalidPayload(
                "template_uid is required".to_string(),
            ));
        }
        validate_checklist_columns(&columns)?;
        let mission_uid = optional_text(&command.args, &["mission_uid", "mission_id"]);
        if let Some(mission_uid) = &mission_uid {
            if !self.missions.contains_key(mission_uid) {
                return Err(RchCoreError::MissionNotFound);
            }
        }
        let now = utc_now_ms();
        let uid = optional_text(&command.args, &["checklist_uid", "uid"])
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        if let Some(existing) = self.checklists.get(&uid) {
            return Ok(existing.clone());
        }
        let start_ts_ms = optional_timestamp_ms(&command.args, &["start_time"])?.unwrap_or(now);
        let checklist = ChecklistRecord {
            uid: uid.clone(),
            mission_uid,
            template_uid,
            template_version: optional_i64(&command.args, "template_version")?.or(Some(1)),
            template_name,
            name,
            description: optional_text_or_empty(&command.args, &["description"])
                .unwrap_or_default(),
            start_ts_ms,
            mode: normalize_checklist_mode(mode)?,
            sync_state: normalize_checklist_sync_state(
                optional_text(&command.args, &["sync_state"])
                    .as_deref()
                    .unwrap_or(default_sync_state),
            )?,
            origin_type: normalize_checklist_origin(
                optional_text(&command.args, &["origin_type"])
                    .as_deref()
                    .unwrap_or("BLANK_TEMPLATE"),
            )?,
            checklist_status: "PENDING".to_string(),
            created_by_team_member_rns_identity: optional_text(
                &command.args,
                &["source_identity", "created_by_team_member_rns_identity"],
            )
            .unwrap_or_else(|| command.source.rns_identity.clone()),
            created_ts_ms: now,
            updated_ts_ms: now,
            uploaded_ts_ms: None,
            progress_percent: 0.0,
            pending_count: 0,
            late_count: 0,
            complete_count: 0,
        };
        self.checklists.insert(uid.clone(), checklist.clone());
        self.insert_checklist_columns(&uid, columns, now);
        self.insert_initial_checklist_tasks_from_args(&uid, command.args.get("tasks"))?;
        self.recompute_checklist_status(&uid)?;
        Ok(self.checklists.get(&uid).cloned().unwrap_or(checklist))
    }

    fn insert_initial_checklist_tasks_from_args(
        &mut self,
        checklist_uid: &str,
        tasks: Option<&Value>,
    ) -> Result<(), RchCoreError> {
        let Some(tasks) = tasks.and_then(Value::as_array) else {
            return Ok(());
        };
        let columns = self.columns_for_checklist(checklist_uid);
        let column_by_order = columns
            .iter()
            .map(|column| (column.display_order, column.column_uid.clone()))
            .collect::<HashMap<_, _>>();
        let column_by_name = columns
            .iter()
            .map(|column| {
                (
                    column.column_name.trim().to_ascii_lowercase(),
                    column.column_uid.clone(),
                )
            })
            .collect::<HashMap<_, _>>();
        for task in tasks {
            let existing_task_uids = self
                .checklist_tasks
                .values()
                .filter(|task| task.checklist_uid == checklist_uid)
                .map(|task| task.task_uid.clone())
                .collect::<HashSet<_>>();
            self.add_checklist_task_row(checklist_uid, task)?;
            let Some(task_uid) = self
                .checklist_tasks
                .values()
                .filter(|task| task.checklist_uid == checklist_uid)
                .find(|task| !existing_task_uids.contains(&task.task_uid))
                .map(|task| task.task_uid.clone())
            else {
                continue;
            };
            let Some(cells) = task.get("cells").and_then(Value::as_array) else {
                continue;
            };
            for cell in cells {
                let column_uid = optional_text(cell, &["column_uid"])
                    .filter(|column_uid| {
                        self.checklist_columns
                            .get(column_uid)
                            .is_some_and(|column| {
                                column.checklist_uid.as_deref() == Some(checklist_uid)
                            })
                    })
                    .or_else(|| {
                        optional_i64(cell, "column_order")
                            .ok()
                            .flatten()
                            .and_then(|order| column_by_order.get(&order).cloned())
                    })
                    .or_else(|| {
                        optional_i64(cell, "display_order")
                            .ok()
                            .flatten()
                            .and_then(|order| column_by_order.get(&order).cloned())
                    })
                    .or_else(|| {
                        optional_text(cell, &["column_name"])
                            .map(|name| name.trim().to_ascii_lowercase())
                            .and_then(|name| column_by_name.get(&name).cloned())
                    });
                let Some(column_uid) = column_uid else {
                    continue;
                };
                self.set_checklist_task_cell(checklist_uid, &task_uid, &column_uid, cell)?;
            }
        }
        Ok(())
    }

    fn insert_checklist_columns(&mut self, checklist_uid: &str, columns: Vec<Value>, now: i64) {
        let columns = if columns.is_empty() {
            default_checklist_columns()
        } else {
            columns
        };
        for (index, column) in columns.into_iter().enumerate() {
            let display_order = column
                .get("display_order")
                .and_then(value_as_i64)
                .unwrap_or_else(|| i64::try_from(index + 1).unwrap_or(1));
            let record = ChecklistColumnRecord {
                column_uid: Uuid::new_v4().simple().to_string(),
                checklist_uid: Some(checklist_uid.to_string()),
                template_uid: None,
                column_name: optional_text(&column, &["column_name"])
                    .unwrap_or_else(|| "Column".to_string()),
                display_order,
                column_type: optional_text(&column, &["column_type"])
                    .and_then(|value| normalize_checklist_column_type(&value).ok())
                    .unwrap_or_else(|| "SHORT_STRING".to_string()),
                column_editable: optional_bool(&column, "column_editable").unwrap_or(true),
                background_color: optional_text_or_empty(&column, &["background_color"])
                    .and_then(none_if_empty),
                text_color: optional_text_or_empty(&column, &["text_color"])
                    .and_then(none_if_empty),
                is_removable: optional_bool(&column, "is_removable").unwrap_or(true),
                system_key: optional_text_or_empty(&column, &["system_key"])
                    .and_then(none_if_empty),
                created_ts_ms: now,
                updated_ts_ms: now,
            };
            self.checklist_columns
                .insert(record.column_uid.clone(), record);
        }
    }

    fn add_checklist_task_row(
        &mut self,
        checklist_uid: &str,
        args: &Value,
    ) -> Result<ChecklistRecord, RchCoreError> {
        let checklist = self.checklists.get(checklist_uid).ok_or_else(|| {
            RchCoreError::InvalidPayload(format!("Checklist '{checklist_uid}' not found"))
        })?;
        let number = optional_i64(args, "number")?.unwrap_or(1);
        let due_relative_minutes = optional_i64(args, "due_relative_minutes")?.or_else(|| {
            if args
                .as_object()
                .is_some_and(|object| object.contains_key("due_dtg"))
            {
                None
            } else {
                Some(number * 30)
            }
        });
        let due_ts_ms = optional_timestamp_ms(args, &["due_dtg"])?.or_else(|| {
            due_relative_minutes.map(|minutes| checklist.start_ts_ms + minutes * 60_000)
        });
        let now = utc_now_ms();
        let task_uid = optional_text(args, &["task_uid"])
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        if let Some(existing_task) = self.checklist_tasks.get_mut(&task_uid) {
            if existing_task.checklist_uid != checklist_uid {
                return Err(RchCoreError::InvalidPayload(format!(
                    "Checklist task '{task_uid}' not found"
                )));
            }
            existing_task.number = number;
            existing_task.due_relative_minutes = due_relative_minutes;
            existing_task.due_ts_ms = due_ts_ms;
            existing_task.legacy_value =
                optional_text_or_empty(args, &["legacy_value"]).and_then(none_if_empty);
            existing_task.notes = optional_text_or_empty(args, &["notes"]).and_then(none_if_empty);
            existing_task.updated_ts_ms = now;
            return self.recompute_checklist_status(checklist_uid);
        }
        let (task_status, is_late) = derive_task_status("PENDING", due_ts_ms, None);
        let task = ChecklistTaskRecord {
            task_uid: task_uid.clone(),
            checklist_uid: checklist_uid.to_string(),
            number,
            user_status: "PENDING".to_string(),
            task_status,
            is_late,
            custom_status: None,
            due_relative_minutes,
            due_ts_ms,
            notes: optional_text_or_empty(args, &["notes"]).and_then(none_if_empty),
            row_background_color: optional_text_or_empty(args, &["row_background_color"])
                .and_then(none_if_empty),
            line_break_enabled: optional_bool(args, "line_break_enabled").unwrap_or(false),
            completed_ts_ms: None,
            completed_by_team_member_rns_identity: None,
            legacy_value: optional_text_or_empty(args, &["legacy_value"]).and_then(none_if_empty),
            created_ts_ms: now,
            updated_ts_ms: now,
        };
        self.checklist_tasks.insert(task_uid.clone(), task);
        for column in self.columns_for_checklist(checklist_uid) {
            let cell = ChecklistCellRecord {
                cell_uid: Uuid::new_v4().simple().to_string(),
                task_uid: task_uid.clone(),
                column_uid: column.column_uid,
                value: None,
                updated_ts_ms: now,
                updated_by_team_member_rns_identity: None,
            };
            self.checklist_cells.insert(cell.cell_uid.clone(), cell);
        }
        self.recompute_checklist_status(checklist_uid)
    }

    fn update_checklist_from_patch(
        &mut self,
        checklist_uid: &str,
        patch: &Value,
    ) -> Result<ChecklistRecord, RchCoreError> {
        let checklist = self.checklists.get_mut(checklist_uid).ok_or_else(|| {
            RchCoreError::InvalidPayload(format!("Checklist '{checklist_uid}' not found"))
        })?;
        if let Some(name) = optional_text(patch, &["name"]) {
            checklist.name = name;
        }
        if let Some(description) = optional_text_or_empty(patch, &["description"]) {
            checklist.description = description;
        }
        if patch.as_object().is_some_and(|object| {
            object.contains_key("mission_uid") || object.contains_key("mission_id")
        }) {
            let mission_uid = optional_text_or_empty(patch, &["mission_uid", "mission_id"])
                .and_then(none_if_empty);
            if let Some(mission_uid) = &mission_uid {
                if !self.missions.contains_key(mission_uid) {
                    return Err(RchCoreError::MissionNotFound);
                }
            }
            checklist.mission_uid = mission_uid;
        }
        if let Some(mode) = optional_text(patch, &["mode"]) {
            checklist.mode = normalize_checklist_mode(&mode)?;
        }
        if let Some(sync_state) = optional_text(patch, &["sync_state"]) {
            checklist.sync_state = normalize_checklist_sync_state(&sync_state)?;
        }
        if let Some(origin_type) = optional_text(patch, &["origin_type"]) {
            checklist.origin_type = normalize_checklist_origin(&origin_type)?;
        }
        if let Some(status) = optional_text(patch, &["checklist_status"]) {
            checklist.checklist_status = normalize_task_status(&status)?;
        }
        checklist.updated_ts_ms = utc_now_ms();
        Ok(checklist.clone())
    }

    fn delete_checklist(&mut self, checklist_uid: &str) -> Result<Value, RchCoreError> {
        let checklist = self.checklists.remove(checklist_uid).ok_or_else(|| {
            RchCoreError::InvalidPayload(format!("Checklist '{checklist_uid}' not found"))
        })?;
        let payload = self.checklist_value(&checklist);
        let task_uids: HashSet<_> = self
            .checklist_tasks
            .values()
            .filter(|task| task.checklist_uid == checklist_uid)
            .map(|task| task.task_uid.clone())
            .collect();
        self.checklist_tasks
            .retain(|_, task| task.checklist_uid != checklist_uid);
        self.checklist_columns
            .retain(|_, column| column.checklist_uid.as_deref() != Some(checklist_uid));
        self.checklist_cells
            .retain(|_, cell| !task_uids.contains(&cell.task_uid));
        self.checklist_feed_publications
            .retain(|_, publication| publication.checklist_uid != checklist_uid);
        self.task_skill_requirements
            .retain(|_, requirement| !task_uids.contains(&requirement.task_uid));
        let assignment_uids: HashSet<_> = self
            .assignments
            .values()
            .filter(|assignment| task_uids.contains(&assignment.task_uid))
            .map(|assignment| assignment.assignment_uid.clone())
            .collect();
        self.assignments
            .retain(|_, assignment| !task_uids.contains(&assignment.task_uid));
        self.assignment_asset_links
            .retain(|(assignment_uid, _)| !assignment_uids.contains(assignment_uid));
        Ok(payload)
    }

    fn upload_checklist(&mut self, checklist_uid: &str) -> Result<ChecklistRecord, RchCoreError> {
        let checklist = self.checklists.get_mut(checklist_uid).ok_or_else(|| {
            RchCoreError::InvalidPayload(format!("Checklist '{checklist_uid}' not found"))
        })?;
        let now = utc_now_ms();
        checklist.sync_state = "SYNCED".to_string();
        checklist.uploaded_ts_ms = Some(now);
        checklist.updated_ts_ms = now;
        Ok(checklist.clone())
    }

    fn publish_checklist_feed(
        &mut self,
        checklist_uid: &str,
        mission_feed_uid: &str,
        command: &MissionCommandEnvelope,
    ) -> Result<ChecklistFeedPublicationRecord, RchCoreError> {
        let checklist = self.checklists.get(checklist_uid).ok_or_else(|| {
            RchCoreError::InvalidPayload(format!("Checklist '{checklist_uid}' not found"))
        })?;
        if checklist.mode == "OFFLINE" && checklist.sync_state != "SYNCED" {
            return Err(RchCoreError::InvalidPayload(
                "Offline checklists must be SYNCED before publication".to_string(),
            ));
        }
        let publication = ChecklistFeedPublicationRecord {
            publication_uid: Uuid::new_v4().simple().to_string(),
            checklist_uid: checklist_uid.to_string(),
            mission_feed_uid: mission_feed_uid.to_string(),
            published_ts_ms: utc_now_ms(),
            published_by_team_member_rns_identity: optional_text(
                &command.args,
                &["source_identity", "published_by_team_member_rns_identity"],
            )
            .unwrap_or_else(|| command.source.rns_identity.clone()),
        };
        self.checklist_feed_publications
            .insert(publication.publication_uid.clone(), publication.clone());
        Ok(publication)
    }

    #[allow(clippy::too_many_lines)]
    fn import_checklist_csv(
        &mut self,
        command: &MissionCommandEnvelope,
    ) -> Result<Value, RchCoreError> {
        let filename = optional_text(&command.args, &["csv_filename"])
            .unwrap_or_else(|| "checklist.csv".to_string());
        let encoded = required_text(&command.args, &["csv_base64"])?;
        let decoded = BASE64_STANDARD
            .decode(encoded)
            .map_err(|_| RchCoreError::InvalidPayload("csv_base64 is invalid".to_string()))?;
        let decoded_text = decode_utf8_ignoring_errors(&decoded);
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(decoded_text.as_bytes());
        let mut rows = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|error| RchCoreError::InvalidPayload(error.to_string()))?;
            let row: Vec<_> = record
                .iter()
                .map(|cell| cell.replace('\u{feff}', "").trim().to_string())
                .collect();
            if row.iter().any(|cell| !cell.is_empty()) {
                rows.push(row);
            }
        }
        if rows.len() < 2 {
            return Err(RchCoreError::InvalidPayload(
                "CSV must include a header row and at least one task row".to_string(),
            ));
        }
        let header_row = &rows[0];
        let task_rows = &rows[1..];
        let max_columns = task_rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(header_row.len()))
            .max()
            .unwrap_or(0);
        if max_columns == 0 {
            return Err(RchCoreError::InvalidPayload(
                "CSV header row is empty".to_string(),
            ));
        }
        let headers: Vec<_> = (0..max_columns)
            .map(|index| {
                header_row
                    .get(index)
                    .filter(|value| !value.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("Column {}", index + 1))
            })
            .collect();
        let due_header_index = headers.iter().position(|header| {
            matches!(
                normalize_csv_header(header).as_str(),
                "due" | "due relative minutes" | "due minutes"
            )
        });
        let mut columns = Vec::new();
        let mut header_display_orders = BTreeMap::new();
        if let Some(due_index) = due_header_index {
            for (index, header) in headers.iter().enumerate() {
                if index == due_index {
                    columns.push(json!({
                        "column_name": if header.is_empty() { "Due" } else { header },
                        "column_type": "RELATIVE_TIME",
                        "column_editable": false,
                        "is_removable": false,
                        "system_key": "DUE_RELATIVE_DTG",
                    }));
                } else {
                    columns.push(json!({
                        "column_name": header,
                        "column_type": "SHORT_STRING",
                        "column_editable": true,
                        "is_removable": true,
                    }));
                    header_display_orders.insert(index, index + 1);
                }
            }
        } else {
            columns.push(json!({
                "column_name": "Due",
                "column_type": "RELATIVE_TIME",
                "column_editable": false,
                "is_removable": false,
                "system_key": "DUE_RELATIVE_DTG",
            }));
            for (index, header) in headers.iter().enumerate() {
                columns.push(json!({
                    "column_name": header,
                    "column_type": "SHORT_STRING",
                    "column_editable": true,
                    "is_removable": true,
                }));
                header_display_orders.insert(index, index + 2);
            }
        }
        let checklist_name = Path::new(&filename)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("Checklist CSV")
            .to_string();
        let create_command = MissionCommandEnvelope {
            command_id: command.command_id.clone(),
            source: command.source.clone(),
            timestamp: command.timestamp.clone(),
            command_type: "checklist.create.online".to_string(),
            args: json!({
                "name": checklist_name,
                "description": format!("Imported from {filename}"),
                "origin_type": "CSV_IMPORT",
                "mission_uid": optional_text(&command.args, &["mission_uid", "mission_id"]),
                "columns": columns,
                "source_identity": optional_text(&command.args, &["source_identity"])
                    .unwrap_or_else(|| command.source.rns_identity.clone()),
            }),
            correlation_id: command.correlation_id.clone(),
            topics: command.topics.clone(),
        };
        let checklist = self.create_checklist_from_args(&create_command, "ONLINE", "SYNCED")?;
        let checklist_uid = checklist.uid.clone();
        let created_columns: HashMap<_, _> = self
            .columns_for_checklist(&checklist_uid)
            .into_iter()
            .map(|column| (column.display_order, column.column_uid))
            .collect();
        let header_column_uids: BTreeMap<_, _> = header_display_orders
            .into_iter()
            .filter_map(|(header_index, display_order)| {
                let display_order = i64::try_from(display_order).ok()?;
                created_columns
                    .get(&display_order)
                    .map(|column_uid| (header_index, column_uid.clone()))
            })
            .collect();
        for (row_index, row) in task_rows.iter().enumerate() {
            let normalized_row: Vec<_> = (0..headers.len())
                .map(|column_index| row.get(column_index).cloned().unwrap_or_default())
                .collect();
            let due_relative_minutes = due_header_index
                .and_then(|index| normalized_row.get(index))
                .and_then(|value| parse_due_minutes(value));
            let legacy_value = normalized_row
                .iter()
                .enumerate()
                .find(|(index, value)| {
                    !value.is_empty() && due_header_index.is_none_or(|due| *index != due)
                })
                .map(|(_, value)| value.clone());
            let row_number = i64::try_from(row_index + 1)
                .map_err(|error| RchCoreError::Decode(error.to_string()))?;
            let updated = self.add_checklist_task_row(
                &checklist_uid,
                &json!({
                    "number": row_number,
                    "due_relative_minutes": due_relative_minutes,
                    "legacy_value": legacy_value,
                }),
            )?;
            let task_uid = self
                .checklist_tasks
                .values()
                .find(|task| task.checklist_uid == updated.uid && task.number == row_number)
                .map(|task| task.task_uid.clone())
                .ok_or_else(|| {
                    RchCoreError::InvalidPayload(
                        "Checklist import failed to create task row".to_string(),
                    )
                })?;
            for (column_index, column_uid) in &header_column_uids {
                if let Some(value) = normalized_row
                    .get(*column_index)
                    .filter(|value| !value.is_empty())
                {
                    self.set_checklist_task_cell(
                        &checklist_uid,
                        &task_uid,
                        column_uid,
                        &json!({
                            "value": value,
                            "updated_by_team_member_rns_identity": optional_text(&command.args, &["source_identity"])
                                .unwrap_or_else(|| command.source.rns_identity.clone()),
                        }),
                    )?;
                }
            }
        }
        let checklist = self
            .checklists
            .get(&checklist_uid)
            .ok_or_else(|| RchCoreError::InvalidPayload("Checklist import failed".to_string()))?;
        Ok(self.checklist_value(checklist))
    }

    fn delete_checklist_task_row(
        &mut self,
        checklist_uid: &str,
        task_uid: &str,
    ) -> Result<ChecklistRecord, RchCoreError> {
        if !self.checklists.contains_key(checklist_uid) {
            return Err(RchCoreError::InvalidPayload(format!(
                "Checklist '{checklist_uid}' not found"
            )));
        }
        let task = self
            .checklist_tasks
            .get(task_uid)
            .filter(|task| task.checklist_uid == checklist_uid)
            .ok_or_else(|| {
                RchCoreError::InvalidPayload(format!("Checklist task '{task_uid}' not found"))
            })?
            .clone();
        self.checklist_tasks.remove(task_uid);
        self.checklist_cells
            .retain(|_, cell| cell.task_uid != task.task_uid);
        self.recompute_checklist_status(checklist_uid)
    }

    fn set_checklist_task_row_style(
        &mut self,
        checklist_uid: &str,
        task_uid: &str,
        args: &Value,
    ) -> Result<ChecklistRecord, RchCoreError> {
        let task = self.checklist_task_mut(checklist_uid, task_uid)?;
        if let Some(color) = optional_text_or_empty(args, &["row_background_color"]) {
            task.row_background_color = none_if_empty(color);
        }
        if let Some(line_break_enabled) = optional_bool(args, "line_break_enabled") {
            task.line_break_enabled = line_break_enabled;
        }
        task.updated_ts_ms = utc_now_ms();
        self.recompute_checklist_status(checklist_uid)
    }

    fn set_checklist_task_cell(
        &mut self,
        checklist_uid: &str,
        task_uid: &str,
        column_uid: &str,
        args: &Value,
    ) -> Result<ChecklistRecord, RchCoreError> {
        self.checklist_task_mut(checklist_uid, task_uid)?;
        if self
            .checklist_columns
            .get(column_uid)
            .is_none_or(|column| column.checklist_uid.as_deref() != Some(checklist_uid))
        {
            return Err(RchCoreError::InvalidPayload(format!(
                "Checklist column '{column_uid}' not found"
            )));
        }
        let now = utc_now_ms();
        let existing_cell_uid = self
            .checklist_cells
            .values()
            .find(|cell| cell.task_uid == task_uid && cell.column_uid == column_uid)
            .map(|cell| cell.cell_uid.clone());
        let cell_uid = existing_cell_uid.unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let cell = ChecklistCellRecord {
            cell_uid: cell_uid.clone(),
            task_uid: task_uid.to_string(),
            column_uid: column_uid.to_string(),
            value: args
                .get("value")
                .and_then(value_as_str)
                .and_then(none_if_empty),
            updated_ts_ms: now,
            updated_by_team_member_rns_identity: optional_text_or_empty(
                args,
                &["updated_by_team_member_rns_identity"],
            )
            .and_then(none_if_empty)
            .or_else(|| {
                self.checklist_cells
                    .values()
                    .find(|cell| cell.task_uid == task_uid && cell.column_uid == column_uid)
                    .and_then(|cell| cell.updated_by_team_member_rns_identity.clone())
            }),
        };
        self.checklist_cells.insert(cell_uid, cell);
        self.recompute_checklist_status(checklist_uid)
    }

    fn set_checklist_task_status(
        &mut self,
        checklist_uid: &str,
        task_uid: &str,
        args: &Value,
    ) -> Result<ChecklistRecord, RchCoreError> {
        if !self.checklists.contains_key(checklist_uid) {
            return Err(RchCoreError::InvalidPayload(format!(
                "Checklist '{checklist_uid}' not found"
            )));
        }
        let task = self
            .checklist_tasks
            .get_mut(task_uid)
            .filter(|task| task.checklist_uid == checklist_uid)
            .ok_or_else(|| {
                RchCoreError::InvalidPayload(format!("Checklist task '{task_uid}' not found"))
            })?;
        let user_status = normalize_checklist_user_status(
            optional_text(args, &["user_status"])
                .as_deref()
                .unwrap_or_default(),
        )?;
        let now = utc_now_ms();
        task.user_status = user_status;
        if task.user_status == "COMPLETE" {
            task.completed_ts_ms = task.completed_ts_ms.or(Some(now));
            task.completed_by_team_member_rns_identity =
                optional_text_or_empty(args, &["changed_by_team_member_rns_identity"])
                    .and_then(none_if_empty)
                    .or_else(|| task.completed_by_team_member_rns_identity.clone());
        } else {
            task.completed_ts_ms = None;
            task.completed_by_team_member_rns_identity = None;
        }
        let (task_status, is_late) =
            derive_task_status(&task.user_status, task.due_ts_ms, task.completed_ts_ms);
        task.task_status = task_status;
        task.is_late = is_late;
        task.updated_ts_ms = now;
        self.recompute_checklist_status(checklist_uid)
    }

    fn checklist_task_mut(
        &mut self,
        checklist_uid: &str,
        task_uid: &str,
    ) -> Result<&mut ChecklistTaskRecord, RchCoreError> {
        if !self.checklists.contains_key(checklist_uid) {
            return Err(RchCoreError::InvalidPayload(format!(
                "Checklist '{checklist_uid}' not found"
            )));
        }
        self.checklist_tasks
            .get_mut(task_uid)
            .filter(|task| task.checklist_uid == checklist_uid)
            .ok_or_else(|| {
                RchCoreError::InvalidPayload(format!("Checklist task '{task_uid}' not found"))
            })
    }

    fn recompute_checklist_status(
        &mut self,
        checklist_uid: &str,
    ) -> Result<ChecklistRecord, RchCoreError> {
        let task_uids: Vec<_> = self
            .checklist_tasks
            .values()
            .filter(|task| task.checklist_uid == checklist_uid)
            .map(|task| task.task_uid.clone())
            .collect();
        let mut pending = 0;
        let mut late = 0;
        let mut complete = 0;
        let mut has_complete_late = false;
        for task_uid in task_uids {
            let task = self
                .checklist_tasks
                .get_mut(&task_uid)
                .ok_or_else(|| RchCoreError::InvalidPayload("task disappeared".to_string()))?;
            let (status, is_late) =
                derive_task_status(&task.user_status, task.due_ts_ms, task.completed_ts_ms);
            task.task_status = status;
            task.is_late = is_late;
            if task.user_status == "COMPLETE" {
                complete += 1;
                has_complete_late |= task.task_status == "COMPLETE_LATE";
            } else {
                pending += 1;
                late += i64::from(task.task_status == "LATE");
            }
        }
        let total = pending + complete;
        let checklist = self.checklists.get_mut(checklist_uid).ok_or_else(|| {
            RchCoreError::InvalidPayload(format!("Checklist '{checklist_uid}' not found"))
        })?;
        checklist.pending_count = pending;
        checklist.late_count = late;
        checklist.complete_count = complete;
        checklist.progress_percent = if total == 0 {
            0.0
        } else {
            let complete = f64::from(u32::try_from(complete).unwrap_or(u32::MAX));
            let total = f64::from(u32::try_from(total).unwrap_or(u32::MAX));
            ((complete / total) * 100.0 * 100.0).round() / 100.0
        };
        checklist.checklist_status = if total == 0 || pending > 0 && late == 0 {
            "PENDING".to_string()
        } else if pending == 0 && has_complete_late {
            "COMPLETE_LATE".to_string()
        } else if pending == 0 {
            "COMPLETE".to_string()
        } else {
            "LATE".to_string()
        };
        checklist.updated_ts_ms = utc_now_ms();
        Ok(checklist.clone())
    }

    fn columns_for_checklist(&self, checklist_uid: &str) -> Vec<ChecklistColumnRecord> {
        let mut columns: Vec<_> = self
            .checklist_columns
            .values()
            .filter(|column| column.checklist_uid.as_deref() == Some(checklist_uid))
            .cloned()
            .collect();
        columns.sort_by_key(|column| column.display_order);
        columns
    }

    fn resolve_parent_uid(args: &Value, current: Option<String>) -> Option<String> {
        let Some(object) = args.as_object() else {
            return current;
        };
        let raw_parent = if object.contains_key("parent_uid") {
            object.get("parent_uid")
        } else {
            object.get("parent")
        };
        let Some(raw_parent) = raw_parent else {
            return current;
        };
        let parent = raw_parent
            .as_object()
            .and_then(|parent| parent.get("uid"))
            .unwrap_or(raw_parent);
        value_as_str(parent).and_then(none_if_empty)
    }

    fn ensure_parent_chain_acyclic(
        &self,
        mission_uid: &str,
        parent_uid: Option<&str>,
    ) -> Result<(), RchCoreError> {
        let Some(parent_uid) = parent_uid else {
            return Ok(());
        };
        if parent_uid == mission_uid {
            return Err(RchCoreError::InvalidPayload(
                "parent_uid cannot reference itself".to_string(),
            ));
        }
        let mut seen = HashSet::from([mission_uid.to_string()]);
        let mut current = Some(parent_uid.to_string());
        while let Some(uid) = current.take() {
            if !seen.insert(uid.clone()) {
                return Err(RchCoreError::InvalidPayload(
                    "mission parent relationship would create a cycle".to_string(),
                ));
            }
            let parent = self.missions.get(&uid).ok_or_else(|| {
                RchCoreError::InvalidPayload(format!("Parent mission '{uid}' not found"))
            })?;
            current.clone_from(&parent.parent_uid);
        }
        Ok(())
    }

    fn create_topic_from_args(&mut self, args: &Value) -> Result<String, RchCoreError> {
        let raw_topic_id = optional_text(args, &["topic_id", "TopicID"]);
        let topic_name = optional_text(args, &["topic_name", "TopicName"])
            .or_else(|| raw_topic_id.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                RchCoreError::InvalidPayload("topic_name and topic_path are required".to_string())
            })?;
        let topic_path = optional_text(args, &["topic_path", "TopicPath"])
            .or_else(|| raw_topic_id.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                RchCoreError::InvalidPayload("topic_name and topic_path are required".to_string())
            })?;
        let topic_description =
            optional_text(args, &["topic_description", "TopicDescription"]).unwrap_or_default();
        let topic_id = raw_topic_id
            .as_deref()
            .and_then(|value| normalize_topic_id(Some(value)))
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let retention = match optional_text(args, &["retention", "Retention"]).as_deref() {
            Some("ephemeral") => RetentionPolicy::Ephemeral,
            _ => RetentionPolicy::Persistent,
        };
        let visibility = match optional_text(args, &["visibility", "Visibility"]).as_deref() {
            Some("restricted") => Visibility::Restricted,
            _ => Visibility::Public,
        };
        let now = utc_now_ms();
        self.topics.insert(
            topic_id.clone(),
            TopicRecord {
                topic_id: topic_id.clone(),
                topic_name,
                topic_path,
                topic_description,
                retention,
                visibility,
                created_ts_ms: now,
                last_activity_ts_ms: now,
            },
        );
        Ok(topic_id)
    }

    fn patch_topic_from_args(&mut self, args: &Value) -> Result<TopicRecord, RchCoreError> {
        let topic_id = required_text(args, &["topic_id", "TopicID", "id", "ID"])?;
        let topic_id = normalize_topic_id(Some(&topic_id))
            .ok_or_else(|| RchCoreError::InvalidPayload("topic_id is required".to_string()))?;
        let topic = self
            .topics
            .get_mut(&topic_id)
            .ok_or(RchCoreError::TopicNotFound)?;
        if let Some(topic_name) = optional_text(args, &["topic_name", "TopicName"]) {
            topic.topic_name = topic_name;
        }
        if let Some(topic_path) = optional_text(args, &["topic_path", "TopicPath"]) {
            topic.topic_path = topic_path;
        }
        if args.as_object().is_some_and(|object| {
            object.contains_key("topic_description") || object.contains_key("TopicDescription")
        }) {
            topic.topic_description =
                optional_text(args, &["topic_description", "TopicDescription"]).unwrap_or_default();
        }
        topic.last_activity_ts_ms = utc_now_ms();
        Ok(topic.clone())
    }

    fn delete_topic_from_args(&mut self, args: &Value) -> Result<TopicRecord, RchCoreError> {
        let topic_id = required_text(args, &["topic_id", "TopicID", "id", "ID"])?;
        let topic_id = normalize_topic_id(Some(&topic_id))
            .ok_or_else(|| RchCoreError::InvalidPayload("topic_id is required".to_string()))?;
        let topic = self
            .topics
            .remove(&topic_id)
            .ok_or(RchCoreError::TopicNotFound)?;
        for attachment in self.file_attachments.values_mut() {
            if attachment
                .topic_id
                .as_deref()
                .and_then(|value| normalize_topic_id(Some(value)))
                .as_deref()
                == Some(topic_id.as_str())
            {
                attachment.topic_id = None;
                attachment.updated_ts_ms = utc_now_ms();
            }
        }
        self.subscribers
            .retain(|(_, subscribed_topic_id), _| subscribed_topic_id != &topic_id);
        Ok(topic)
    }

    fn subscription_from_args(
        command: &MissionCommandEnvelope,
        args: &Value,
    ) -> Result<(String, String, Option<i64>, Value), RchCoreError> {
        let topic_id = required_text(args, &["topic_id", "TopicID", "topic_path", "TopicPath"])?;
        let topic_id = normalize_topic_id(Some(&topic_id))
            .ok_or_else(|| RchCoreError::InvalidPayload("TopicID is required".to_string()))?;
        let subscriber_id = optional_text(
            args,
            &[
                "subscriber_id",
                "SubscriberID",
                "destination",
                "Destination",
            ],
        )
        .unwrap_or_else(|| command.source.rns_identity.clone());
        let metadata = args.get("metadata").cloned().unwrap_or(Value::Null);
        let reject_tests = args
            .as_object()
            .and_then(|object| {
                object
                    .get("reject_tests")
                    .or_else(|| object.get("RejectTests"))
            })
            .and_then(value_as_i64);
        Ok((topic_id, subscriber_id, reject_tests, metadata))
    }

    fn subscribe(
        &mut self,
        topic_id: &str,
        subscriber_id: &str,
        reject_tests: Option<i64>,
        metadata: Value,
    ) -> Result<(), RchCoreError> {
        if !self.topics.contains_key(topic_id) {
            return Err(RchCoreError::TopicNotFound);
        }
        let subscriber_id = normalize_subscriber_node_id(Some(subscriber_id))
            .ok_or_else(|| RchCoreError::InvalidPayload("Destination is required".to_string()))?;
        let now = utc_now_ms();
        let key = (subscriber_id.clone(), topic_id.to_string());
        self.subscribers
            .entry(key)
            .and_modify(|record| {
                record.last_seen_ts_ms = now;
                record.reject_tests = reject_tests;
                record.metadata = metadata.clone();
            })
            .or_insert_with(|| SubscriberRecord {
                node_id: subscriber_id,
                topic_id: topic_id.to_string(),
                first_seen_ts_ms: now,
                last_seen_ts_ms: now,
                reject_tests,
                metadata,
            });
        Ok(())
    }

    fn patch_subscriber_from_args(
        &mut self,
        args: &Value,
    ) -> Result<SubscriberRecord, RchCoreError> {
        let subscriber_id = required_text(args, &["subscriber_id", "SubscriberID"])?;
        let subscriber_id = normalize_subscriber_node_id(Some(&subscriber_id))
            .ok_or_else(|| RchCoreError::InvalidPayload("SubscriberID is required".to_string()))?;
        let existing_key = self
            .subscribers
            .keys()
            .find(|(node_id, _)| node_id == &subscriber_id)
            .cloned()
            .ok_or(RchCoreError::TopicNotFound)?;
        let mut subscriber = self
            .subscribers
            .get(&existing_key)
            .cloned()
            .ok_or(RchCoreError::TopicNotFound)?;

        if let Some(destination) = optional_text(args, &["destination", "Destination"]) {
            subscriber.node_id =
                normalize_subscriber_node_id(Some(&destination)).ok_or_else(|| {
                    RchCoreError::InvalidPayload("Destination is required".to_string())
                })?;
        }
        if let Some(topic_id) = optional_text(args, &["topic_id", "TopicID"]) {
            let topic_id = normalize_topic_id(Some(&topic_id))
                .ok_or_else(|| RchCoreError::InvalidPayload("TopicID is required".to_string()))?;
            if !self.topics.contains_key(&topic_id) {
                return Err(RchCoreError::TopicNotFound);
            }
            subscriber.topic_id = topic_id;
        }
        if let Some(metadata) = args.get("metadata").or_else(|| args.get("Metadata")) {
            subscriber.metadata = metadata.clone();
        }
        if args.as_object().is_some_and(|object| {
            object.contains_key("reject_tests") || object.contains_key("RejectTests")
        }) {
            subscriber.reject_tests =
                optional_i64(args, "reject_tests")?.or(optional_i64(args, "RejectTests")?);
        }
        subscriber.last_seen_ts_ms = utc_now_ms();

        let updated_key = (subscriber.node_id.clone(), subscriber.topic_id.clone());
        if updated_key != existing_key && self.subscribers.contains_key(&updated_key) {
            return Err(RchCoreError::InvalidPayload(
                "normalized subscriber destination/topic pair already exists".to_string(),
            ));
        }
        self.subscribers.remove(&existing_key);
        self.subscribers.insert(updated_key, subscriber.clone());
        Ok(subscriber)
    }

    fn delete_subscriber_from_args(
        &mut self,
        args: &Value,
    ) -> Result<SubscriberRecord, RchCoreError> {
        let subscriber_id = required_text(args, &["subscriber_id", "SubscriberID", "id", "ID"])?;
        let subscriber_id = normalize_subscriber_node_id(Some(&subscriber_id))
            .ok_or_else(|| RchCoreError::InvalidPayload("SubscriberID is required".to_string()))?;
        let key = self
            .subscribers
            .keys()
            .find(|(node_id, _)| node_id == &subscriber_id)
            .cloned()
            .ok_or(RchCoreError::TopicNotFound)?;
        self.subscribers
            .remove(&key)
            .ok_or(RchCoreError::TopicNotFound)
    }

    fn record_message(
        &mut self,
        message_id: String,
        content: &str,
        topic_id: Option<&str>,
        destination: Option<&str>,
        sender: &str,
    ) -> Result<EventEnvelope, RchCoreError> {
        if content.trim().is_empty() {
            return Err(RchCoreError::InvalidPayload(
                "content is required".to_string(),
            ));
        }
        let normalized_topic = normalize_topic_id(topic_id);
        let destination = normalize_hash(destination);
        let mode = if normalized_topic.is_some() {
            DeliveryMode::Fanout
        } else if destination.is_some() {
            DeliveryMode::Targeted
        } else {
            DeliveryMode::Broadcast
        };
        if let Some(topic_id) = &normalized_topic {
            let topic = self
                .topics
                .get_mut(topic_id)
                .ok_or(RchCoreError::TopicNotFound)?;
            topic.last_activity_ts_ms = utc_now_ms();
        }
        self.messages.push(MessageRecord {
            message_id: message_id.clone(),
            topic_id: normalized_topic.clone(),
            destination: destination.clone(),
            sender: normalize_hash(Some(sender)).unwrap_or_else(|| sender.to_string()),
            content: content.to_string(),
            delivery_mode: mode,
            delivery_method: "direct".to_string(),
            delivery_policy_reason: "default_direct".to_string(),
            delivery_state: "queued".to_string(),
            delivery_metadata: json!({}),
            created_ts_ms: utc_now_ms(),
            attachments: Vec::new(),
        });
        Ok(Self::event_by_parts(
            "mission.message.sent",
            message_id,
            None,
            json!({
                "sent": true,
            "content": content,
                "topic_id": normalized_topic,
                "destination": destination,
                "mode": format!("{mode:?}").to_ascii_lowercase(),
            }),
        ))
    }

    fn event(event_type: &str, command: &MissionCommandEnvelope, payload: Value) -> EventEnvelope {
        Self::event_by_parts(
            event_type,
            command.command_id.clone(),
            command.correlation_id.clone(),
            payload,
        )
    }

    fn event_by_parts(
        event_type: &str,
        command_id: String,
        correlation_id: Option<String>,
        payload: Value,
    ) -> EventEnvelope {
        EventEnvelope {
            event_id: None,
            source: None,
            timestamp: None,
            event_type: event_type.to_string(),
            command_id: Some(command_id),
            correlation_id,
            topics: Vec::new(),
            payload,
        }
    }

    fn record_audit_event(&mut self, command: &MissionCommandEnvelope, event: &EventEnvelope) {
        self.audit_events.push(MissionAuditEvent {
            event_id: Uuid::new_v4().simple().to_string(),
            event_type: event.event_type.clone(),
            command_type: command.command_type.clone(),
            command_id: command.command_id.clone(),
            source_identity: normalize_hash(Some(&command.source.rns_identity))
                .unwrap_or_else(|| command.source.rns_identity.clone()),
            timestamp_ms: utc_now_ms(),
            payload: event.payload.clone(),
        });
    }

    fn recent_audit_event_values(&self, limit: usize) -> Vec<Value> {
        self.audit_events
            .iter()
            .rev()
            .take(limit)
            .map(|event| {
                json!({
                    "id": event.event_id,
                    "timestamp": millis_to_rfc3339(event.timestamp_ms),
                    "type": "mission_command_processed",
                    "message": "mission command processed",
                    "metadata": {
                        "command_id": event.command_id,
                        "command_type": if event.command_type.is_empty() {
                            Value::Null
                        } else {
                            json!(event.command_type)
                        },
                        "identity": event.source_identity,
                        "event_type": event.event_type,
                    },
                    "origin": Value::Null,
                })
            })
            .collect()
    }

    fn marker_values(&self) -> Vec<Value> {
        self.markers()
            .into_iter()
            .map(|marker| Self::marker_value(&marker))
            .collect()
    }

    fn marker_value(marker: &MarkerRecord) -> Value {
        let timestamp = millis_to_rfc3339(marker.updated_ts_ms);
        json!({
            "object_destination_hash": marker.object_destination_hash,
            "origin_rch": marker.origin_rch,
            "type": marker.marker_type,
            "name": marker.name,
            "category": marker.category,
            "symbol": marker.symbol,
            "notes": marker.notes,
            "position": { "lat": marker.lat, "lon": marker.lon },
            "time": timestamp,
            "stale_at": timestamp,
            "created_at": millis_to_rfc3339(marker.created_ts_ms),
            "updated_at": timestamp,
        })
    }

    fn zone_values(&self) -> Vec<Value> {
        self.zones()
            .into_iter()
            .map(|zone| Self::zone_value(&zone))
            .collect()
    }

    fn zone_value(zone: &ZoneRecord) -> Value {
        json!({
            "zone_id": zone.zone_id,
            "name": zone.name,
            "points": zone.points.iter().map(|point| {
                json!({ "lat": point.lat, "lon": point.lon })
            }).collect::<Vec<_>>(),
            "created_at": millis_to_rfc3339(zone.created_ts_ms),
            "updated_at": millis_to_rfc3339(zone.updated_ts_ms),
        })
    }

    fn limited_mission_values(&self, args: &Value) -> Vec<Value> {
        let limit = args
            .get("limit")
            .and_then(value_as_i64)
            .map_or(200, |value| {
                usize::try_from(value.clamp(1, 2000)).unwrap_or(200)
            });
        self.missions()
            .into_iter()
            .take(limit)
            .map(|mission| self.mission_value_with_args(&mission, args))
            .collect()
    }

    fn mission_value_with_args(&self, mission: &MissionRecord, args: &Value) -> Value {
        let mut payload = self.mission_value(mission);
        let expand = mission_expand_values(args);
        if expand.contains("topic") {
            payload["topic"] = mission
                .topic_id
                .as_ref()
                .and_then(|topic_id| self.topics.get(topic_id))
                .map_or(Value::Null, |topic| {
                    Self::topic_value(topic, self.subscribers(&topic.topic_id).len())
                });
        }
        if expand.contains("teams") || expand.contains("team_members") || expand.contains("assets")
        {
            let team_uids = self.mission_team_ids_for_expand(&mission.uid);
            if expand.contains("teams") {
                payload["teams"] = Value::Array(
                    self.teams()
                        .into_iter()
                        .filter(|team| team_uids.iter().any(|uid| uid == &team.uid))
                        .map(|team| Self::team_value(&team))
                        .collect(),
                );
            }
            let team_members = self
                .team_members()
                .into_iter()
                .filter(|member| {
                    member
                        .team_uid
                        .as_ref()
                        .is_some_and(|team_uid| team_uids.iter().any(|uid| uid == team_uid))
                })
                .collect::<Vec<_>>();
            if expand.contains("team_members") {
                payload["team_members"] = Value::Array(
                    team_members
                        .iter()
                        .map(Self::team_member_value)
                        .collect::<Vec<_>>(),
                );
            }
            if expand.contains("assets") {
                let member_uids = team_members
                    .iter()
                    .map(|member| member.uid.as_str())
                    .collect::<Vec<_>>();
                payload["assets"] = Value::Array(
                    self.assets()
                        .into_iter()
                        .filter(|asset| {
                            asset
                                .team_member_uid
                                .as_deref()
                                .is_some_and(|uid| member_uids.contains(&uid))
                        })
                        .map(|asset| Self::asset_value(&asset))
                        .collect(),
                );
            }
        }
        if expand.contains("mission_changes") {
            payload["mission_changes"] = json!(self.mission_change_values(Some(&mission.uid)));
        }
        if expand.contains("log_entries") {
            payload["log_entries"] = json!(self.log_entry_values(Some(&mission.uid), None));
        }
        if expand.contains("assignments") {
            payload["assignments"] = json!(self.assignment_values(Some(&mission.uid), None));
        }
        if expand.contains("checklists") {
            payload["checklists"] = Value::Array(
                self.checklists()
                    .into_iter()
                    .filter(|checklist| checklist.mission_uid.as_deref() == Some(&mission.uid))
                    .map(|checklist| self.checklist_value(&checklist))
                    .collect(),
            );
        }
        if expand.contains("mission_rde") {
            payload["mission_rde"] = json!({
                "mission_uid": mission.uid,
                "role": mission.mission_rde_role,
                "updated_at": millis_to_rfc3339(mission.updated_ts_ms),
            });
        }
        payload
    }

    fn mission_value(&self, mission: &MissionRecord) -> Value {
        json!({
            "uid": mission.uid,
            "mission_name": mission.mission_name,
            "description": mission.description,
            "topic_id": mission.topic_id,
            "path": mission.path,
            "classification": mission.classification,
            "tool": mission.tool,
            "keywords": mission.keywords,
            "parent_uid": mission.parent_uid,
            "children": self.mission_children(&mission.uid),
            "feeds": mission.feeds,
            "zones": self.mission_zone_ids(&mission.uid),
            "markers": self.mission_marker_ids(&mission.uid),
            "password_hash": mission.password_hash,
            "default_role": mission.default_role,
            "mission_priority": mission.mission_priority,
            "mission_status": mission.mission_status,
            "owner_role": mission.owner_role,
            "token": mission.token,
            "invite_only": mission.invite_only,
            "expiration": mission.expiration,
            "mission_rde_role": mission.mission_rde_role,
            "created_at": millis_to_rfc3339(mission.created_ts_ms),
            "updated_at": millis_to_rfc3339(mission.updated_ts_ms),
        })
    }

    fn mission_team_ids_for_expand(&self, mission_uid: &str) -> Vec<String> {
        let mut team_uids: Vec<_> = self
            .mission_team_links
            .iter()
            .filter(|(linked_mission_uid, _)| linked_mission_uid == mission_uid)
            .map(|(_, team_uid)| team_uid.clone())
            .collect();
        team_uids.extend(
            self.teams
                .values()
                .filter(|team| team.mission_uid.as_deref() == Some(mission_uid))
                .map(|team| team.uid.clone()),
        );
        dedupe_non_empty(team_uids)
    }

    fn mission_zone_ids(&self, mission_uid: &str) -> Vec<String> {
        let mut zones: Vec<_> = self
            .mission_zone_links
            .iter()
            .filter(|(linked_mission_uid, _)| linked_mission_uid == mission_uid)
            .map(|(_, zone_id)| zone_id.clone())
            .collect();
        zones.sort();
        zones
    }

    fn mission_marker_ids(&self, mission_uid: &str) -> Vec<String> {
        let mut markers: Vec<_> = self
            .mission_marker_links
            .iter()
            .filter(|(linked_mission_uid, _)| linked_mission_uid == mission_uid)
            .map(|(_, marker_id)| marker_id.clone())
            .collect();
        markers.sort();
        markers
    }

    fn mission_children(&self, mission_uid: &str) -> Vec<String> {
        let mut children: Vec<_> = self
            .missions
            .values()
            .filter(|mission| mission.parent_uid.as_deref() == Some(mission_uid))
            .map(|mission| mission.uid.clone())
            .collect();
        children.sort();
        children
    }

    fn mission_change_values(&self, mission_uid: Option<&str>) -> Vec<Value> {
        self.mission_changes()
            .into_iter()
            .filter(|change| mission_uid.is_none_or(|uid| change.mission_uid == uid))
            .map(|change| Self::mission_change_value(&change))
            .collect()
    }

    fn mission_change_value(change: &MissionChangeRecord) -> Value {
        json!({
            "uid": change.uid,
            "mission_uid": change.mission_uid,
            "mission_id": change.mission_uid,
            "name": change.name,
            "team_member_rns_identity": change.team_member_rns_identity,
            "timestamp": millis_to_rfc3339(change.timestamp_ms),
            "notes": change.notes,
            "change_type": change.change_type,
            "is_federated_change": change.is_federated_change,
            "hashes": change.hashes,
            "delta": change.delta,
        })
    }

    fn log_entry_values(&self, mission_uid: Option<&str>, marker_ref: Option<&str>) -> Vec<Value> {
        self.log_entries()
            .into_iter()
            .filter(|entry| mission_uid.is_none_or(|uid| entry.mission_uid == uid))
            .filter(|entry| {
                marker_ref.is_none_or(|marker_ref| {
                    entry.content_hashes.iter().any(|hash| hash == marker_ref)
                })
            })
            .map(|entry| Self::log_entry_value(&entry))
            .collect()
    }

    fn log_entry_value(entry: &LogEntryRecord) -> Value {
        json!({
            "entry_uid": entry.entry_uid,
            "mission_uid": entry.mission_uid,
            "callsign": entry.callsign,
            "content": entry.content,
            "server_time": millis_to_rfc3339(entry.server_time_ms),
            "client_time": entry.client_time,
            "content_hashes": entry.content_hashes,
            "keywords": entry.keywords,
            "mecp": mecp_log_entry_value(&entry.content),
            "created_at": millis_to_rfc3339(entry.created_ts_ms),
            "updated_at": millis_to_rfc3339(entry.updated_ts_ms),
        })
    }

    fn team_values(&self, mission_uid: Option<&str>) -> Vec<Value> {
        self.teams()
            .into_iter()
            .filter(|team| {
                mission_uid.is_none_or(|uid| team.mission_uids.iter().any(|item| item == uid))
            })
            .map(|team| Self::team_value(&team))
            .collect()
    }

    fn eam_values(
        &self,
        team_uid: Option<&str>,
        overall_status: Option<&str>,
    ) -> Result<Vec<Value>, RchCoreError> {
        let overall_status = overall_status
            .map(|status| normalize_eam_status(Some(status)))
            .transpose()?;
        Ok(self
            .eam_snapshots()
            .into_iter()
            .filter(|eam| eam.deleted_ts_ms.is_none() && !eam_is_expired(eam))
            .filter(|eam| team_uid.is_none_or(|uid| eam.team_uid == uid))
            .filter(|eam| {
                overall_status
                    .as_ref()
                    .is_none_or(|status| &eam.overall_status == status)
            })
            .map(|eam| Self::eam_value(&eam))
            .collect())
    }

    fn eam_value(eam: &EamSnapshotRecord) -> Value {
        json!({
            "eam_uid": eam.eam_uid,
            "callsign": eam.callsign,
            "group_name": eam.group_name,
            "team_member_uid": eam.team_member_uid,
            "team_uid": eam.team_uid,
            "reported_by": eam.reported_by,
            "reported_at": millis_to_rfc3339(eam.reported_ts_ms),
            "overall_status": eam.overall_status,
            "security_status": eam.security_status,
            "capability_status": eam.capability_status,
            "preparedness_status": eam.preparedness_status,
            "medical_status": eam.medical_status,
            "mobility_status": eam.mobility_status,
            "comms_status": eam.comms_status,
            "notes": eam.notes,
            "confidence": eam.confidence,
            "ttl_seconds": eam.ttl_seconds,
            "source": eam.source,
        })
    }

    fn eam_group_name_for_team(&self, team_uid: &str) -> Option<String> {
        if let Some(color) = canonical_team_color_for_uid(team_uid) {
            return Some(color.to_string());
        }
        self.teams.get(team_uid).and_then(|team| {
            team.color
                .clone()
                .or_else(|| none_if_empty(team.team_name.clone()))
        })
    }

    fn eam_team_summary(&self, team_uid: &str) -> Result<Value, RchCoreError> {
        if !self.teams.contains_key(team_uid) {
            return Err(RchCoreError::InvalidPayload(format!(
                "Team '{team_uid}' not found"
            )));
        }
        let rows: Vec<_> = self
            .eam_snapshots
            .values()
            .filter(|eam| eam.team_uid == team_uid)
            .collect();
        let active: Vec<_> = rows
            .iter()
            .copied()
            .filter(|eam| eam.deleted_ts_ms.is_none() && !eam_is_expired(eam))
            .collect();
        let green_total = active
            .iter()
            .filter(|eam| eam.overall_status == "Green")
            .count();
        let yellow_total = active
            .iter()
            .filter(|eam| eam.overall_status == "Yellow")
            .count();
        let red_total = active
            .iter()
            .filter(|eam| eam.overall_status == "Red")
            .count();
        let overall_status = if red_total > 0 {
            Some("Red")
        } else if yellow_total > 0 {
            Some("Yellow")
        } else if green_total > 0 {
            Some("Green")
        } else {
            None
        };
        let updated_at_ms = rows
            .iter()
            .map(|eam| eam.updated_ts_ms)
            .max()
            .unwrap_or_else(utc_now_ms);
        Ok(json!({
            "team_uid": team_uid,
            "total": rows.len(),
            "active_total": active.len(),
            "deleted_total": rows.len().saturating_sub(active.len()),
            "overall_status": overall_status,
            "green_total": green_total,
            "yellow_total": yellow_total,
            "red_total": red_total,
            "updated_at_ms": updated_at_ms,
        }))
    }

    fn team_value(team: &TeamRecord) -> Value {
        json!({
            "uid": team.uid,
            "mission_uid": team.mission_uid,
            "mission_uids": team.mission_uids,
            "color": team.color,
            "team_name": team.team_name,
            "team_description": team.team_description,
            "created_at": millis_to_rfc3339(team.created_ts_ms),
            "updated_at": millis_to_rfc3339(team.updated_ts_ms),
        })
    }

    fn team_mission_ids(&self, team_uid: &str) -> Vec<String> {
        let mut mission_uids: Vec<_> = self
            .mission_team_links
            .iter()
            .filter(|(_, linked_team_uid)| linked_team_uid == team_uid)
            .map(|(mission_uid, _)| mission_uid.clone())
            .collect();
        if let Some(team) = self
            .teams
            .get(team_uid)
            .and_then(|team| team.mission_uid.clone())
        {
            mission_uids.push(team);
        }
        dedupe_non_empty(mission_uids)
    }

    fn team_member_values(&self, team_uid: Option<&str>) -> Vec<Value> {
        self.team_members()
            .into_iter()
            .filter(|member| team_uid.is_none_or(|uid| member.team_uid.as_deref() == Some(uid)))
            .map(|member| Self::team_member_value(&member))
            .collect()
    }

    fn team_member_value(member: &TeamMemberRecord) -> Value {
        json!({
            "uid": member.uid,
            "team_uid": member.team_uid,
            "rns_identity": member.rns_identity,
            "display_name": member.display_name,
            "icon": member.icon,
            "role": member.role,
            "callsign": member.callsign,
            "freq": member.freq,
            "email": member.email,
            "phone": member.phone,
            "modulation": member.modulation,
            "availability": member.availability,
            "certifications": member.certifications,
            "last_active": member.last_active,
            "client_identities": member.client_identities,
            "created_at": millis_to_rfc3339(member.created_ts_ms),
            "updated_at": millis_to_rfc3339(member.updated_ts_ms),
        })
    }

    fn team_member_subject_values(&self, mission_uid: Option<&str>) -> Vec<Value> {
        self.team_members()
            .into_iter()
            .filter(|member| {
                let mission_uids = member
                    .team_uid
                    .as_deref()
                    .map_or_else(Vec::new, |team_uid| self.team_mission_ids(team_uid));
                mission_uid.is_none_or(|uid| mission_uids.iter().any(|item| item == uid))
            })
            .map(|member| {
                let mission_uids = member
                    .team_uid
                    .as_deref()
                    .map_or_else(Vec::new, |team_uid| self.team_mission_ids(team_uid));
                let team_name = member
                    .team_uid
                    .as_deref()
                    .and_then(|team_uid| self.teams.get(team_uid))
                    .map(|team| team.team_name.clone());
                json!({
                    "subject_type": "team_member",
                    "subject_id": member.uid,
                    "team_member_uid": member.uid,
                    "rns_identity": member.rns_identity,
                    "display_name": member.display_name,
                    "team_uid": member.team_uid,
                    "team_name": team_name,
                    "client_identities": member.client_identities,
                    "mission_uids": mission_uids,
                })
            })
            .collect()
    }

    fn mission_access_assignment_values(
        &self,
        mission_uid: Option<&str>,
        subject_type: Option<&str>,
        subject_id: Option<&str>,
    ) -> Vec<Value> {
        let normalized_subject_type =
            subject_type.and_then(|value| normalize_subject_type(value).ok());
        let normalized_subject_id = normalized_subject_type.as_deref().zip(subject_id).and_then(
            |(subject_type, subject_id)| normalize_subject_id(subject_type, subject_id).ok(),
        );
        self.mission_access_assignment_records()
            .into_iter()
            .filter(|assignment| {
                mission_uid.is_none_or(|uid| assignment.mission_uid == uid)
                    && normalized_subject_type
                        .as_ref()
                        .is_none_or(|value| assignment.subject_type == *value)
                    && normalized_subject_id
                        .as_ref()
                        .is_none_or(|value| assignment.subject_id == *value)
            })
            .map(|assignment| Self::mission_access_assignment_value(&assignment))
            .collect()
    }

    fn mission_access_assignment_value(assignment: &MissionAccessAssignment) -> Value {
        json!({
            "mission_uid": assignment.mission_uid,
            "subject_type": assignment.subject_type,
            "subject_id": assignment.subject_id,
            "role": assignment.role,
        })
    }

    fn asset_values(&self, team_member_uid: Option<&str>) -> Vec<Value> {
        self.assets()
            .into_iter()
            .filter(|asset| {
                team_member_uid.is_none_or(|uid| asset.team_member_uid.as_deref() == Some(uid))
            })
            .map(|asset| Self::asset_value(&asset))
            .collect()
    }

    fn asset_value(asset: &AssetRecord) -> Value {
        json!({
            "asset_uid": asset.asset_uid,
            "team_member_uid": asset.team_member_uid,
            "name": asset.name,
            "asset_type": asset.asset_type,
            "serial_number": asset.serial_number,
            "status": asset.status,
            "location": asset.location,
            "notes": asset.notes,
            "created_at": millis_to_rfc3339(asset.created_ts_ms),
            "updated_at": millis_to_rfc3339(asset.updated_ts_ms),
        })
    }

    fn skill_values(&self) -> Vec<Value> {
        self.skills()
            .into_iter()
            .map(|skill| Self::skill_value(&skill))
            .collect()
    }

    fn skill_value(skill: &SkillRecord) -> Value {
        json!({
            "skill_uid": skill.skill_uid,
            "name": skill.name,
            "category": skill.category,
            "description": skill.description,
            "proficiency_scale": skill.proficiency_scale,
            "created_at": millis_to_rfc3339(skill.created_ts_ms),
            "updated_at": millis_to_rfc3339(skill.updated_ts_ms),
        })
    }

    fn team_member_skill_values(&self, identity: Option<&str>) -> Vec<Value> {
        self.team_member_skills()
            .into_iter()
            .filter(|record| {
                identity.is_none_or(|identity| record.team_member_rns_identity == identity)
            })
            .map(|record| Self::team_member_skill_value(&record))
            .collect()
    }

    fn team_member_skill_value(record: &TeamMemberSkillRecord) -> Value {
        json!({
            "uid": record.uid,
            "team_member_rns_identity": record.team_member_rns_identity,
            "skill_uid": record.skill_uid,
            "level": record.level,
            "validated_by": record.validated_by,
            "validated_at": record.validated_at,
            "expires_at": record.expires_at,
        })
    }

    fn task_skill_requirement_values(&self, task_uid: Option<&str>) -> Vec<Value> {
        self.task_skill_requirements()
            .into_iter()
            .filter(|record| task_uid.is_none_or(|task_uid| record.task_uid == task_uid))
            .map(|record| Self::task_skill_requirement_value(&record))
            .collect()
    }

    fn task_skill_requirement_value(record: &TaskSkillRequirementRecord) -> Value {
        json!({
            "uid": record.uid,
            "task_uid": record.task_uid,
            "skill_uid": record.skill_uid,
            "minimum_level": record.minimum_level,
            "is_mandatory": record.is_mandatory,
        })
    }

    fn assignment_values(&self, mission_uid: Option<&str>, task_uid: Option<&str>) -> Vec<Value> {
        self.assignments()
            .into_iter()
            .filter(|record| {
                mission_uid.is_none_or(|mission_uid| record.mission_uid == mission_uid)
            })
            .filter(|record| task_uid.is_none_or(|task_uid| record.task_uid == task_uid))
            .map(|record| self.assignment_value(&record))
            .collect()
    }

    fn assignment_value(&self, assignment: &AssignmentRecord) -> Value {
        let assets = self.assignment_assets(&assignment.assignment_uid, &assignment.assets);
        json!({
            "assignment_uid": assignment.assignment_uid,
            "mission_uid": assignment.mission_uid,
            "task_uid": assignment.task_uid,
            "team_member_rns_identity": assignment.team_member_rns_identity,
            "assigned_by": assignment.assigned_by,
            "assigned_at": millis_to_rfc3339(assignment.assigned_ts_ms),
            "due_dtg": assignment.due_dtg,
            "status": assignment.status,
            "notes": assignment.notes,
            "assets": assets,
        })
    }

    fn assignment_assets(&self, assignment_uid: &str, fallback_assets: &[String]) -> Vec<String> {
        let mut assets: Vec<_> = self
            .assignment_asset_links
            .iter()
            .filter(|(linked_assignment_uid, _)| linked_assignment_uid == assignment_uid)
            .map(|(_, asset_uid)| asset_uid.clone())
            .collect();
        assets.sort();
        if assets.is_empty() {
            fallback_assets.to_vec()
        } else {
            assets
        }
    }

    fn checklist_values(&self) -> Vec<Value> {
        self.checklists()
            .into_iter()
            .map(|checklist| self.checklist_value(&checklist))
            .collect()
    }

    fn checklist_template_values(&self) -> Vec<Value> {
        self.checklist_templates()
            .into_iter()
            .map(|template| self.checklist_template_value(&template))
            .collect()
    }

    fn checklist_template_value(&self, template: &ChecklistTemplateRecord) -> Value {
        json!({
            "uid": template.uid,
            "template_name": template.template_name,
            "description": template.description,
            "created_at": millis_to_rfc3339(template.created_ts_ms),
            "created_by_team_member_rns_identity": template.created_by_team_member_rns_identity,
            "updated_at": millis_to_rfc3339(template.updated_ts_ms),
            "source_template_uid": template.source_template_uid,
            "server_only": template.server_only,
            "columns": self.template_column_values(&template.uid),
        })
    }

    fn checklist_value(&self, checklist: &ChecklistRecord) -> Value {
        json!({
            "uid": checklist.uid,
            "mission_id": checklist.mission_uid,
            "mission_uid": checklist.mission_uid,
            "template_uid": checklist.template_uid,
            "template_version": checklist.template_version,
            "template_name": checklist.template_name,
            "name": checklist.name,
            "description": checklist.description,
            "start_time": millis_to_rfc3339(checklist.start_ts_ms),
            "mode": checklist.mode,
            "sync_state": checklist.sync_state,
            "origin_type": checklist.origin_type,
            "checklist_status": checklist.checklist_status,
            "created_at": millis_to_rfc3339(checklist.created_ts_ms),
            "created_by_team_member_rns_identity": checklist.created_by_team_member_rns_identity,
            "updated_at": millis_to_rfc3339(checklist.updated_ts_ms),
            "uploaded_at": checklist.uploaded_ts_ms.map(millis_to_rfc3339),
            "progress_percent": checklist.progress_percent,
            "counts": {
                "pending_count": checklist.pending_count,
                "late_count": checklist.late_count,
                "complete_count": checklist.complete_count,
            },
            "columns": self.checklist_column_values(&checklist.uid),
            "tasks": self.checklist_task_values(&checklist.uid),
            "feed_publications": self.checklist_feed_publication_values(&checklist.uid),
        })
    }

    fn checklist_feed_publication_values(&self, checklist_uid: &str) -> Vec<Value> {
        let mut publications: Vec<_> = self
            .checklist_feed_publications
            .values()
            .filter(|publication| publication.checklist_uid == checklist_uid)
            .cloned()
            .collect();
        publications.sort_by(|left, right| {
            right
                .published_ts_ms
                .cmp(&left.published_ts_ms)
                .then(left.publication_uid.cmp(&right.publication_uid))
        });
        publications
            .into_iter()
            .map(|publication| checklist_feed_publication_value(&publication))
            .collect()
    }

    fn checklist_column_values(&self, checklist_uid: &str) -> Vec<Value> {
        self.columns_for_checklist(checklist_uid)
            .into_iter()
            .map(|column| checklist_column_record_value(&column))
            .collect()
    }

    fn checklist_task_values(&self, checklist_uid: &str) -> Vec<Value> {
        let mut tasks: Vec<_> = self
            .checklist_tasks
            .values()
            .filter(|task| task.checklist_uid == checklist_uid)
            .cloned()
            .collect();
        tasks.sort_by_key(|task| task.number);
        tasks
            .into_iter()
            .map(|task| {
                json!({
                    "task_uid": task.task_uid,
                    "number": task.number,
                    "user_status": task.user_status,
                    "task_status": task.task_status,
                    "is_late": task.is_late,
                    "custom_status": task.custom_status,
                    "due_relative_minutes": task.due_relative_minutes,
                    "due_dtg": task.due_ts_ms.map(millis_to_rfc3339),
                    "notes": task.notes,
                    "row_background_color": task.row_background_color,
                    "line_break_enabled": task.line_break_enabled,
                    "completed_at": task.completed_ts_ms.map(millis_to_rfc3339),
                    "completed_by_team_member_rns_identity": task.completed_by_team_member_rns_identity,
                    "legacy_value": task.legacy_value,
                    "cells": self.checklist_cell_values(&task.task_uid),
                })
            })
            .collect()
    }

    fn checklist_cell_values(&self, task_uid: &str) -> Vec<Value> {
        let mut cells: Vec<_> = self
            .checklist_cells
            .values()
            .filter(|cell| cell.task_uid == task_uid)
            .cloned()
            .collect();
        cells.sort_by(|left, right| left.column_uid.cmp(&right.column_uid));
        cells
            .into_iter()
            .map(|cell| {
                json!({
                    "cell_uid": cell.cell_uid,
                    "task_uid": cell.task_uid,
                    "column_uid": cell.column_uid,
                    "value": cell.value,
                    "updated_at": millis_to_rfc3339(cell.updated_ts_ms),
                    "updated_by_team_member_rns_identity": cell.updated_by_team_member_rns_identity,
                })
            })
            .collect()
    }

    fn topic_values(&self) -> Vec<Value> {
        self.topics()
            .into_iter()
            .map(|topic| Self::topic_value(&topic, self.subscribers(&topic.topic_id).len()))
            .collect()
    }

    fn topic_value(topic: &TopicRecord, subscriber_count: usize) -> Value {
        json!({
            "topic_id": topic.topic_id,
            "TopicID": topic.topic_id,
            "topic_name": topic.topic_name,
            "TopicName": topic.topic_name,
            "topic_path": topic.topic_path,
            "TopicPath": topic.topic_path,
            "topic_description": topic.topic_description,
            "TopicDescription": topic.topic_description,
            "visibility": topic.visibility,
            "retention": topic.retention,
            "subscriber_count": subscriber_count,
            "SubscriberCount": subscriber_count,
            "created_ts": topic.created_ts_ms,
            "last_activity_ts": topic.last_activity_ts_ms,
        })
    }

    fn subscriber_value(subscriber: &SubscriberRecord) -> Value {
        json!({
            "subscriber_id": subscriber.node_id,
            "SubscriberID": subscriber.node_id,
            "destination": subscriber.node_id,
            "Destination": subscriber.node_id,
            "topic_id": subscriber.topic_id,
            "TopicID": subscriber.topic_id,
            "reject_tests": subscriber.reject_tests,
            "RejectTests": subscriber.reject_tests,
            "metadata": subscriber.metadata,
            "Metadata": subscriber.metadata,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissionSyncResponse {
    pub content: String,
    pub fields: BTreeMap<i64, Value>,
}

impl MissionSyncResponse {
    #[must_use]
    pub fn results(result: CommandResultEnvelope) -> Self {
        Self::results_with_event(result, None)
    }

    #[must_use]
    pub fn rem_results(result: CommandResultEnvelope) -> Self {
        Self::rem_results_with_event(result, None)
    }

    #[must_use]
    pub fn results_with_event(result: CommandResultEnvelope, event: Option<Value>) -> Self {
        Self::with_content_and_event("mission-sync", result, event)
    }

    #[must_use]
    pub fn rem_results_with_event(result: CommandResultEnvelope, event: Option<Value>) -> Self {
        Self::with_content_and_event("", result, event)
    }

    fn with_content_and_event(
        content: impl Into<String>,
        result: CommandResultEnvelope,
        event: Option<Value>,
    ) -> Self {
        let mut fields = BTreeMap::from([(
            r3akt_profile_rch::FIELD_RESULTS,
            serde_json::to_value(result).unwrap_or(Value::Null),
        )]);
        if let Some(event) = event {
            fields.insert(r3akt_profile_rch::FIELD_EVENT, event);
        }
        Self {
            content: content.into(),
            fields,
        }
    }

    #[must_use]
    pub fn results_field(&self) -> Option<&Value> {
        self.fields.get(&r3akt_profile_rch::FIELD_RESULTS)
    }

    #[must_use]
    pub fn event_field(&self) -> Option<&Value> {
        self.fields.get(&r3akt_profile_rch::FIELD_EVENT)
    }
}

#[must_use]
pub fn is_supported_mission_command(command_type: &str) -> bool {
    if is_supported_rem_registry_command(command_type) {
        return true;
    }
    matches!(
        command_type,
        "mission.join"
            | "mission.leave"
            | "mission.pause"
            | "mission.resume"
            | "mission.nick"
            | "mission.users"
            | "mission.events.list"
            | "mission.marker.list"
            | "mission.marker.create"
            | "mission.marker.position.patch"
            | "mission.marker.patch"
            | "mission.marker.delete"
            | "mission.zone.list"
            | "mission.zone.create"
            | "mission.zone.patch"
            | "mission.zone.delete"
            | "mission.registry.mission.upsert"
            | "mission.registry.mission.get"
            | "mission.registry.mission.list"
            | "mission.registry.mission.patch"
            | "mission.registry.mission.delete"
            | "mission.registry.mission.parent.set"
            | "mission.registry.mission.zone.link"
            | "mission.registry.mission.zone.unlink"
            | "mission.registry.mission.marker.link"
            | "mission.registry.mission.marker.unlink"
            | "mission.registry.mission.rde.set"
            | "mission.registry.mission_change.upsert"
            | "mission.registry.mission_change.list"
            | "mission.registry.log_entry.upsert"
            | "mission.registry.log_entry.list"
            | "mission.registry.eam.list"
            | "mission.registry.eam.upsert"
            | "mission.registry.eam.get"
            | "mission.registry.eam.latest"
            | "mission.registry.eam.delete"
            | "mission.registry.eam.team.summary"
            | "mission.registry.team.upsert"
            | "mission.registry.team.get"
            | "mission.registry.team.list"
            | "mission.registry.team.delete"
            | "mission.registry.team.mission.link"
            | "mission.registry.team.mission.unlink"
            | "mission.registry.team_member.upsert"
            | "mission.registry.team_member.get"
            | "mission.registry.team_member.list"
            | "mission.registry.team_member.delete"
            | "mission.registry.team_member.client.link"
            | "mission.registry.team_member.client.unlink"
            | "mission.registry.asset.upsert"
            | "mission.registry.asset.get"
            | "mission.registry.asset.list"
            | "mission.registry.asset.delete"
            | "mission.registry.skill.upsert"
            | "mission.registry.skill.list"
            | "mission.registry.team_member_skill.upsert"
            | "mission.registry.team_member_skill.list"
            | "mission.registry.task_skill_requirement.upsert"
            | "mission.registry.task_skill_requirement.list"
            | "mission.registry.assignment.upsert"
            | "mission.registry.assignment.list"
            | "mission.registry.assignment.asset.set"
            | "mission.registry.assignment.asset.link"
            | "mission.registry.assignment.asset.unlink"
            | "mission.registry.rights.subjects.list"
            | "mission.registry.rights.mission_access.assign"
            | "mission.registry.rights.mission_access.list"
            | "mission.registry.rights.mission_access.revoke"
            | "ListTopic"
            | "topic.list"
            | "CreateTopic"
            | "topic.create"
            | "topic.patch"
            | "topic.delete"
            | "SubscribeTopic"
            | "CreateSubscriber"
            | "AddSubscriber"
            | "topic.subscribe"
            | "topic.subscriber.patch"
            | "topic.subscriber.delete"
            | "mission.message.send"
            | "PublishMessage"
    )
}

#[must_use]
pub fn is_supported_rem_registry_command(command_type: &str) -> bool {
    matches!(
        command_type,
        "rem.registry.mode.set" | "rem.registry.peers.list" | "rem.registry.team_peers.list"
    )
}

#[must_use]
pub fn is_supported_checklist_command(command_type: &str) -> bool {
    matches!(
        command_type,
        "checklist.template.list"
            | "checklist.template.get"
            | "checklist.template.create"
            | "checklist.template.update"
            | "checklist.template.clone"
            | "checklist.template.delete"
            | "checklist.create.online"
            | "checklist.create.offline"
            | "checklist.list.active"
            | "checklist.get"
            | "checklist.update"
            | "checklist.delete"
            | "checklist.join"
            | "checklist.upload"
            | "checklist.import.csv"
            | "checklist.feed.publish"
            | "checklist.task.row.add"
            | "checklist.task.row.delete"
            | "checklist.task.row.style.set"
            | "checklist.task.cell.set"
            | "checklist.task.status.set"
    )
}

#[must_use]
pub fn required_capability(command_type: &str) -> Option<&'static str> {
    match command_type {
        "mission.join" => Some("mission.join"),
        "mission.leave" | "mission.pause" | "mission.resume" | "mission.nick" => {
            Some("mission.leave")
        }
        "mission.users" | "mission.events.list" => Some("mission.audit.read"),
        "mission.marker.list" => Some("mission.content.read"),
        "mission.marker.create"
        | "mission.marker.position.patch"
        | "mission.marker.patch"
        | "mission.marker.delete" => Some("mission.content.write"),
        "mission.zone.list" => Some("mission.zone.read"),
        "mission.zone.create" | "mission.zone.patch" => Some("mission.zone.write"),
        "mission.zone.delete" => Some("mission.zone.delete"),
        "mission.registry.mission.upsert"
        | "mission.registry.mission.patch"
        | "mission.registry.mission.delete"
        | "mission.registry.mission.parent.set"
        | "mission.registry.mission.rde.set" => Some("mission.registry.mission.write"),
        "mission.registry.mission.zone.link" | "mission.registry.mission.zone.unlink" => {
            Some("mission.zone.write")
        }
        "mission.registry.mission.marker.link" | "mission.registry.mission.marker.unlink" => {
            Some("mission.content.write")
        }
        "mission.registry.mission.get" | "mission.registry.mission.list" => {
            Some("mission.registry.mission.read")
        }
        "mission.registry.mission_change.upsert" | "mission.registry.log_entry.upsert" => {
            Some("mission.registry.log.write")
        }
        "mission.registry.mission_change.list" | "mission.registry.log_entry.list" => {
            Some("mission.registry.log.read")
        }
        "mission.registry.eam.upsert" | "mission.registry.eam.delete" => {
            Some("mission.registry.status.write")
        }
        "mission.registry.eam.list"
        | "mission.registry.eam.get"
        | "mission.registry.eam.latest"
        | "mission.registry.eam.team.summary" => Some("mission.registry.status.read"),
        "mission.registry.team.upsert"
        | "mission.registry.team.delete"
        | "mission.registry.team.mission.link"
        | "mission.registry.team.mission.unlink"
        | "mission.registry.team_member.upsert"
        | "mission.registry.team_member.delete"
        | "mission.registry.team_member.client.link"
        | "mission.registry.team_member.client.unlink"
        | "mission.registry.rights.mission_access.assign"
        | "mission.registry.rights.mission_access.revoke" => Some("mission.registry.team.write"),
        "mission.registry.team.get"
        | "mission.registry.team.list"
        | "mission.registry.team_member.get"
        | "mission.registry.team_member.list" => Some("mission.registry.team.read"),
        "mission.registry.asset.upsert" | "mission.registry.asset.delete" => {
            Some("mission.registry.asset.write")
        }
        "mission.registry.asset.get" | "mission.registry.asset.list" => {
            Some("mission.registry.asset.read")
        }
        "mission.registry.skill.upsert"
        | "mission.registry.team_member_skill.upsert"
        | "mission.registry.task_skill_requirement.upsert" => Some("mission.registry.skill.write"),
        "mission.registry.skill.list"
        | "mission.registry.team_member_skill.list"
        | "mission.registry.task_skill_requirement.list" => Some("mission.registry.skill.read"),
        "mission.registry.assignment.upsert"
        | "mission.registry.assignment.asset.set"
        | "mission.registry.assignment.asset.link"
        | "mission.registry.assignment.asset.unlink" => Some("mission.registry.assignment.write"),
        "mission.registry.assignment.list" => Some("mission.registry.assignment.read"),
        "mission.registry.rights.subjects.list" | "mission.registry.rights.mission_access.list" => {
            Some("mission.registry.team.read")
        }
        "ListTopic" | "topic.list" => Some("topic.read"),
        "CreateTopic" | "topic.create" => Some("topic.create"),
        "topic.patch" | "topic.subscriber.patch" => Some("topic.write"),
        "topic.delete" | "topic.subscriber.delete" => Some("topic.delete"),
        "SubscribeTopic" | "CreateSubscriber" | "AddSubscriber" | "topic.subscribe" => {
            Some("topic.subscribe")
        }
        "mission.message.send" | "PublishMessage" => Some("mission.message.send"),
        _ => None,
    }
}

#[must_use]
pub fn checklist_required_capability(command_type: &str) -> Option<&'static str> {
    match command_type {
        "checklist.template.list" | "checklist.template.get" => Some("checklist.template.read"),
        "checklist.template.create" | "checklist.template.update" | "checklist.template.clone" => {
            Some("checklist.template.write")
        }
        "checklist.template.delete" => Some("checklist.template.delete"),
        "checklist.list.active" | "checklist.get" => Some("checklist.read"),
        "checklist.join" => Some("checklist.join"),
        "checklist.upload" => Some("checklist.upload"),
        "checklist.feed.publish" => Some("checklist.feed.publish"),
        "checklist.create.online"
        | "checklist.create.offline"
        | "checklist.update"
        | "checklist.delete"
        | "checklist.import.csv"
        | "checklist.task.row.add"
        | "checklist.task.row.delete"
        | "checklist.task.row.style.set"
        | "checklist.task.cell.set"
        | "checklist.task.status.set" => Some("checklist.write"),
        _ => None,
    }
}

#[must_use]
pub fn checklist_event_type(command_type: &str) -> &'static str {
    match command_type {
        "checklist.template.create" | "checklist.template.clone" => "checklist.template.created",
        "checklist.template.delete" => "checklist.template.deleted",
        "checklist.template.list" | "checklist.template.get" | "checklist.template.update" => {
            "checklist.template.updated"
        }
        "checklist.create.online" | "checklist.create.offline" => "checklist.created",
        "checklist.list.active"
        | "checklist.get"
        | "checklist.task.row.add"
        | "checklist.task.row.delete"
        | "checklist.task.row.style.set"
        | "checklist.task.cell.set" => "checklist.progress.changed",
        "checklist.delete" => "checklist.deleted",
        "checklist.join" => "checklist.joined",
        "checklist.upload" => "checklist.uploaded",
        "checklist.import.csv" => "checklist.imported.csv",
        "checklist.feed.publish" => "checklist.feed.published",
        "checklist.task.status.set" => "checklist.task.status.changed",
        _ => "checklist.updated",
    }
}

fn checklist_update_added_shareable_mission(
    previous_mission_uid: Option<&str>,
    checklist_payload: &Value,
) -> bool {
    let Some(current_mission_uid) =
        optional_text(checklist_payload, &["mission_uid", "mission_id"]).and_then(none_if_empty)
    else {
        return false;
    };
    if previous_mission_uid
        .map(str::trim)
        .is_some_and(|previous| previous == current_mission_uid)
    {
        return false;
    }
    let mode = optional_text(checklist_payload, &["mode"]).unwrap_or_default();
    let sync_state = optional_text(checklist_payload, &["sync_state"]).unwrap_or_default();
    mode.eq_ignore_ascii_case("ONLINE") && sync_state.eq_ignore_ascii_case("SYNCED")
}

pub fn protocol_destination_for_rch(
    topic_id: Option<&str>,
    destination: Option<&str>,
) -> Result<Destination, RchCoreError> {
    match classify_delivery_mode(topic_id, destination)? {
        DeliveryMode::Targeted => Ok(Destination::Node(NodeId::new(
            normalize_hash(destination).unwrap_or_default(),
        ))),
        DeliveryMode::Fanout => Ok(Destination::Topic(Topic::new(
            normalize_topic_id(topic_id).unwrap_or_default(),
        ))),
        DeliveryMode::Broadcast => Ok(Destination::Broadcast),
    }
}

#[must_use]
pub fn normalize_topic_id(value: Option<&str>) -> Option<String> {
    let text = value?.trim();
    if text.is_empty() {
        return None;
    }
    Uuid::parse_str(text)
        .map(|uuid| uuid.simple().to_string())
        .ok()
        .or_else(|| Some(text.to_string()))
}

fn normalize_subscriber_node_id(value: Option<&str>) -> Option<String> {
    let text = value?.trim();
    if text.is_empty() {
        return None;
    }
    if text.len() == 32 && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(text.to_ascii_lowercase())
    } else {
        Some(text.to_string())
    }
}

fn normalize_subscriber_topic_id(value: &str) -> Option<String> {
    let text = value.trim();
    if text.is_empty() {
        Some(String::new())
    } else {
        normalize_topic_id(Some(text))
    }
}

#[must_use]
pub fn normalize_topic_id_bytes(value: &[u8]) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    std::str::from_utf8(value)
        .ok()
        .and_then(|text| normalize_topic_id(Some(text)))
        .or_else(|| Some(bytes_to_lower_hex(value)))
}

#[must_use]
pub fn normalize_hash(value: Option<&str>) -> Option<String> {
    let text = value?.trim().to_ascii_lowercase();
    if text.is_empty() { None } else { Some(text) }
}

fn normalize_announce_capabilities(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let value = value.trim().to_ascii_lowercase();
        if value.is_empty() || !seen.insert(value.clone()) {
            continue;
        }
        normalized.push(value);
    }
    normalized
}

fn classify_client_type(announce_capabilities: &[String]) -> String {
    let capabilities: HashSet<_> = announce_capabilities.iter().map(String::as_str).collect();
    if capabilities.contains("r3akt") && capabilities.contains("emergencymessages") {
        "rem".to_string()
    } else {
        "generic_lxmf".to_string()
    }
}

fn normalize_rem_mode(value: &str) -> Result<String, RchCoreError> {
    let mode = value.trim().to_ascii_lowercase();
    match mode.as_str() {
        "autonomous" | "semi_autonomous" | "connected" => Ok(mode),
        _ => Err(RchCoreError::InvalidPayload(
            "mode must be one of: autonomous, semi_autonomous, connected".to_string(),
        )),
    }
}

fn normalize_subject_type(value: &str) -> Result<String, RchCoreError> {
    let subject_type = value.trim().to_ascii_lowercase();
    match subject_type.as_str() {
        "identity" | "team_member" => Ok(subject_type),
        _ => Err(RchCoreError::InvalidPayload(
            "subject_type must be one of: identity, team_member".to_string(),
        )),
    }
}

fn normalize_subject_id(subject_type: &str, subject_id: &str) -> Result<String, RchCoreError> {
    let subject_id = required_non_empty(subject_id, "subject_id")?;
    if subject_type == "identity" {
        Ok(subject_id.to_ascii_lowercase())
    } else {
        Ok(subject_id)
    }
}

fn normalize_scope_type(value: &str) -> Result<String, RchCoreError> {
    let scope_type = value.trim().to_ascii_lowercase();
    match scope_type.as_str() {
        "" | "global" => Ok("global".to_string()),
        "mission" => Ok(scope_type),
        _ => Err(RchCoreError::InvalidPayload(
            "scope_type must be one of: global, mission".to_string(),
        )),
    }
}

fn normalize_scope_id(scope_type: &str, scope_id: &str) -> String {
    let scope_id = scope_id.trim();
    if scope_type == "global" {
        String::new()
    } else {
        scope_id.to_string()
    }
}

#[must_use]
pub fn normalize_hash_bytes(value: &[u8]) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(bytes_to_lower_hex(value))
    }
}

#[must_use]
pub fn normalize_message_id(value: Option<&str>) -> String {
    value
        .and_then(|text| {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(Uuid::parse_str(trimmed).map_or_else(
                    |_| trimmed.to_ascii_lowercase(),
                    |uuid| uuid.simple().to_string(),
                ))
            }
        })
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string())
}

fn bytes_to_lower_hex(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn encode_msgpack<T>(value: &T) -> Result<Vec<u8>, RchCoreError>
where
    T: Serialize,
{
    rmp_serde::to_vec_named(value).map_err(|error| RchCoreError::Encode(error.to_string()))
}

fn decode_msgpack<T>(bytes: &[u8]) -> Result<T, RchCoreError>
where
    T: DeserializeOwned,
{
    rmp_serde::from_slice(bytes).map_err(|error| RchCoreError::Decode(error.to_string()))
}

fn sqlite_table_has_column(
    connection: &Connection,
    table: &str,
    column: &str,
) -> Result<bool, RchCoreError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn sqlite_table_exists(connection: &Connection, table: &str) -> Result<bool, RchCoreError> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get::<_, bool>(0),
    )?;
    Ok(exists)
}

fn sql_text(value: impl Into<String>) -> SqlValue {
    SqlValue::Text(value.into())
}

fn sql_optional_text(value: Option<&str>) -> SqlValue {
    value.map_or(SqlValue::Null, |value| SqlValue::Text(value.to_string()))
}

fn sql_integer(value: i64) -> SqlValue {
    SqlValue::Integer(value)
}

fn sql_key_string(columns: &[(&'static str, SqlValue)]) -> String {
    columns
        .iter()
        .map(|(_, value)| match value {
            SqlValue::Null => "<null>".to_string(),
            SqlValue::Integer(value) => value.to_string(),
            SqlValue::Real(value) => value.to_string(),
            SqlValue::Text(value) => value.clone(),
            SqlValue::Blob(value) => bytes_to_lower_hex(value),
        })
        .collect::<Vec<_>>()
        .join("\u{1f}")
}

fn delete_payload_row(
    transaction: &Transaction<'_>,
    table: &str,
    key_columns: Vec<(&'static str, SqlValue)>,
) -> Result<(), RchCoreError> {
    let where_clause = key_columns
        .iter()
        .enumerate()
        .map(|(index, (column, _))| format!("{column} = ?{}", index + 1))
        .collect::<Vec<_>>()
        .join(" AND ");
    let values = key_columns
        .into_iter()
        .map(|(_, value)| value)
        .collect::<Vec<_>>();
    transaction.execute(
        &format!("DELETE FROM {table} WHERE {where_clause}"),
        params_from_iter(values),
    )?;
    Ok(())
}

fn upsert_payload_row<T>(
    transaction: &Transaction<'_>,
    table: &str,
    indexed_columns: Vec<(&'static str, SqlValue)>,
    payload: &T,
) -> Result<(), RchCoreError>
where
    T: Serialize,
{
    let column_names = indexed_columns
        .iter()
        .map(|(column, _)| *column)
        .chain(std::iter::once("payload"))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=indexed_columns.len() + 1)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut values = indexed_columns
        .into_iter()
        .map(|(_, value)| value)
        .collect::<Vec<_>>();
    values.push(SqlValue::Blob(encode_msgpack(payload)?));
    transaction.execute(
        &format!("INSERT OR REPLACE INTO {table} ({column_names}) VALUES ({placeholders})"),
        params_from_iter(values),
    )?;
    Ok(())
}

fn save_payload_delta<T, KeyFn, UpsertFn>(
    transaction: &Transaction<'_>,
    table: &str,
    before: &[T],
    after: &[T],
    key_columns: KeyFn,
    indexed_columns: UpsertFn,
) -> Result<(), RchCoreError>
where
    T: Serialize,
    KeyFn: Fn(&T) -> Vec<(&'static str, SqlValue)> + Copy,
    UpsertFn: Fn(&T) -> Vec<(&'static str, SqlValue)> + Copy,
{
    let before_keys = before
        .iter()
        .map(|record| {
            let columns = key_columns(record);
            (sql_key_string(&columns), columns)
        })
        .collect::<HashMap<_, _>>();
    let after_key_set = after
        .iter()
        .map(|record| sql_key_string(&key_columns(record)))
        .collect::<HashSet<_>>();
    for (key, columns) in before_keys {
        if !after_key_set.contains(key.as_str()) {
            delete_payload_row(transaction, table, columns)?;
        }
    }
    for record in after {
        upsert_payload_row(transaction, table, indexed_columns(record), record)?;
    }
    Ok(())
}

fn clear_snapshot_tables(
    transaction: &Transaction<'_>,
    options: SnapshotSaveOptions,
) -> Result<(), RchCoreError> {
    for table in [
        "rch_topics",
        "rch_subscribers",
        "rch_messages",
        "rch_clients",
        "rch_identity_announces",
        "rch_identity_states",
        "rch_identity_rem_modes",
        "rch_audit_events",
        "rch_system_events",
        "rch_telemetry_records",
        "rch_markers",
        "rch_zones",
        "rch_missions",
        "rch_mission_changes",
        "rch_log_entries",
        "rch_file_attachments",
        "rch_eam_snapshots",
        "rch_teams",
        "rch_mission_team_links",
        "rch_mission_zone_links",
        "rch_mission_marker_links",
        "rch_team_members",
        "rch_team_member_client_links",
        "rch_assets",
        "rch_skills",
        "rch_team_member_skills",
        "rch_task_skill_requirements",
        "rch_assignments",
        "rch_assignment_asset_links",
        "rch_checklists",
        "rch_checklist_templates",
        "rch_checklist_columns",
        "rch_checklist_tasks",
        "rch_checklist_cells",
        "rch_checklist_feed_publications",
        "rch_command_results",
        "rch_identity_capabilities",
        "rch_mission_access_assignments",
        "rch_subject_operation_rights",
        "rch_settings",
    ] {
        if options.preserve_identity_announces && table == "rch_identity_announces" {
            continue;
        }
        transaction.execute(&format!("DELETE FROM {table}"), [])?;
    }
    Ok(())
}

fn save_topic_snapshot_tables(
    transaction: &Transaction<'_>,
    snapshot: &RchCoreSnapshot,
    options: SnapshotSaveOptions,
) -> Result<(), RchCoreError> {
    for topic in &snapshot.topics {
        transaction.execute(
            "INSERT INTO rch_topics (topic_id, payload) VALUES (?1, ?2)",
            params![topic.topic_id, encode_msgpack(topic)?],
        )?;
    }
    for subscriber in &snapshot.subscribers {
        transaction.execute(
            "INSERT INTO rch_subscribers (node_id, topic_id, payload) VALUES (?1, ?2, ?3)",
            params![
                subscriber.node_id,
                subscriber.topic_id,
                encode_msgpack(subscriber)?
            ],
        )?;
    }
    for message in &snapshot.messages {
        let projection = message_queue_projection(message);
        transaction.execute(
            "INSERT INTO rch_messages (
                message_id,
                payload,
                delivery_state,
                dispatch_status,
                next_attempt_at_ts_ms,
                attempts,
                priority,
                batch_id,
                created_ts_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                message.message_id,
                encode_msgpack(message)?,
                projection.delivery_state,
                projection.dispatch_status,
                projection.next_attempt_at_ts_ms,
                projection.attempts,
                projection.priority,
                projection.batch_id,
                projection.created_ts_ms,
            ],
        )?;
    }
    for client in &snapshot.clients {
        transaction.execute(
            "INSERT INTO rch_clients (identity, payload) VALUES (?1, ?2)",
            params![client.identity, encode_msgpack(client)?],
        )?;
    }
    if !options.preserve_identity_announces {
        for announce in &snapshot.identity_announces {
            transaction.execute(
                "INSERT INTO rch_identity_announces (destination_hash, payload) VALUES (?1, ?2)",
                params![announce.destination_hash, encode_msgpack(announce)?],
            )?;
        }
    }
    for state in &snapshot.identity_states {
        transaction.execute(
            "INSERT INTO rch_identity_states (identity, payload) VALUES (?1, ?2)",
            params![state.identity, encode_msgpack(state)?],
        )?;
    }
    for rem_mode in &snapshot.identity_rem_modes {
        transaction.execute(
            "INSERT INTO rch_identity_rem_modes (identity, payload) VALUES (?1, ?2)",
            params![rem_mode.identity, encode_msgpack(rem_mode)?],
        )?;
    }
    for event in &snapshot.audit_events {
        transaction.execute(
            "INSERT INTO rch_audit_events (event_id, payload) VALUES (?1, ?2)",
            params![event.event_id, encode_msgpack(event)?],
        )?;
    }
    for event in &snapshot.system_events {
        transaction.execute(
            "INSERT INTO rch_system_events (event_id, payload) VALUES (?1, ?2)",
            params![event.event_id, encode_msgpack(event)?],
        )?;
    }
    for record in &snapshot.telemetry_records {
        transaction.execute(
            "INSERT INTO rch_telemetry_records (peer_destination, timestamp_s, payload) VALUES (?1, ?2, ?3)",
            params![
                record.peer_destination,
                record.timestamp_s,
                encode_msgpack(record)?
            ],
        )?;
    }
    Ok(())
}

fn save_registry_delta_tables(
    transaction: &Transaction<'_>,
    before: &RchCoreSnapshot,
    after: &RchCoreSnapshot,
) -> Result<(), RchCoreError> {
    save_payload_delta(
        transaction,
        "rch_markers",
        &before.markers,
        &after.markers,
        |record| {
            vec![(
                "object_destination_hash",
                sql_text(&record.object_destination_hash),
            )]
        },
        |record| {
            vec![(
                "object_destination_hash",
                sql_text(&record.object_destination_hash),
            )]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_zones",
        &before.zones,
        &after.zones,
        |record| vec![("zone_id", sql_text(&record.zone_id))],
        |record| vec![("zone_id", sql_text(&record.zone_id))],
    )?;
    save_payload_delta(
        transaction,
        "rch_missions",
        &before.missions,
        &after.missions,
        |record| vec![("uid", sql_text(&record.uid))],
        |record| vec![("uid", sql_text(&record.uid))],
    )?;
    save_payload_delta(
        transaction,
        "rch_mission_changes",
        &before.mission_changes,
        &after.mission_changes,
        |record| vec![("uid", sql_text(&record.uid))],
        |record| {
            vec![
                ("uid", sql_text(&record.uid)),
                ("mission_uid", sql_text(&record.mission_uid)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_log_entries",
        &before.log_entries,
        &after.log_entries,
        |record| vec![("entry_uid", sql_text(&record.entry_uid))],
        |record| vec![("entry_uid", sql_text(&record.entry_uid))],
    )?;
    save_payload_delta(
        transaction,
        "rch_eam_snapshots",
        &before.eam_snapshots,
        &after.eam_snapshots,
        |record| vec![("eam_uid", sql_text(&record.eam_uid))],
        |record| {
            vec![
                ("eam_uid", sql_text(&record.eam_uid)),
                ("callsign", sql_text(&record.callsign)),
                ("team_member_uid", sql_text(&record.team_member_uid)),
                ("team_uid", sql_text(&record.team_uid)),
                (
                    "deleted_ts_ms",
                    record
                        .deleted_ts_ms
                        .map_or(SqlValue::Null, SqlValue::Integer),
                ),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_teams",
        &before.teams,
        &after.teams,
        |record| vec![("uid", sql_text(&record.uid))],
        |record| vec![("uid", sql_text(&record.uid))],
    )?;
    save_payload_delta(
        transaction,
        "rch_mission_team_links",
        &before.mission_team_links,
        &after.mission_team_links,
        |record| {
            vec![
                ("mission_uid", sql_text(&record.mission_uid)),
                ("team_uid", sql_text(&record.team_uid)),
            ]
        },
        |record| {
            vec![
                ("mission_uid", sql_text(&record.mission_uid)),
                ("team_uid", sql_text(&record.team_uid)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_mission_zone_links",
        &before.mission_zone_links,
        &after.mission_zone_links,
        |record| {
            vec![
                ("mission_uid", sql_text(&record.mission_uid)),
                ("zone_id", sql_text(&record.zone_id)),
            ]
        },
        |record| {
            vec![
                ("mission_uid", sql_text(&record.mission_uid)),
                ("zone_id", sql_text(&record.zone_id)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_mission_marker_links",
        &before.mission_marker_links,
        &after.mission_marker_links,
        |record| {
            vec![
                ("mission_uid", sql_text(&record.mission_uid)),
                ("marker_id", sql_text(&record.marker_id)),
            ]
        },
        |record| {
            vec![
                ("mission_uid", sql_text(&record.mission_uid)),
                ("marker_id", sql_text(&record.marker_id)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_team_members",
        &before.team_members,
        &after.team_members,
        |record| vec![("uid", sql_text(&record.uid))],
        |record| vec![("uid", sql_text(&record.uid))],
    )?;
    save_payload_delta(
        transaction,
        "rch_team_member_client_links",
        &before.team_member_client_links,
        &after.team_member_client_links,
        |record| {
            vec![
                ("team_member_uid", sql_text(&record.team_member_uid)),
                ("client_identity", sql_text(&record.client_identity)),
            ]
        },
        |record| {
            vec![
                ("team_member_uid", sql_text(&record.team_member_uid)),
                ("client_identity", sql_text(&record.client_identity)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_assets",
        &before.assets,
        &after.assets,
        |record| vec![("asset_uid", sql_text(&record.asset_uid))],
        |record| vec![("asset_uid", sql_text(&record.asset_uid))],
    )?;
    save_skill_assignment_delta_tables(transaction, before, after)?;
    save_authorization_delta_tables(transaction, before, after)?;
    save_audit_event_delta_tables(transaction, before, after)?;
    save_payload_delta(
        transaction,
        "rch_command_results",
        &before.command_results,
        &after.command_results,
        |record| vec![("command_id", sql_text(&record.command_id))],
        |record| vec![("command_id", sql_text(&record.command_id))],
    )?;
    Ok(())
}

fn save_audit_event_delta_tables(
    transaction: &Transaction<'_>,
    before: &RchCoreSnapshot,
    after: &RchCoreSnapshot,
) -> Result<(), RchCoreError> {
    save_payload_delta(
        transaction,
        "rch_audit_events",
        &before.audit_events,
        &after.audit_events,
        |record| vec![("event_id", sql_text(&record.event_id))],
        |record| vec![("event_id", sql_text(&record.event_id))],
    )
}

fn save_skill_assignment_delta_tables(
    transaction: &Transaction<'_>,
    before: &RchCoreSnapshot,
    after: &RchCoreSnapshot,
) -> Result<(), RchCoreError> {
    save_payload_delta(
        transaction,
        "rch_skills",
        &before.skills,
        &after.skills,
        |record| vec![("skill_uid", sql_text(&record.skill_uid))],
        |record| vec![("skill_uid", sql_text(&record.skill_uid))],
    )?;
    save_payload_delta(
        transaction,
        "rch_team_member_skills",
        &before.team_member_skills,
        &after.team_member_skills,
        |record| {
            vec![
                (
                    "team_member_rns_identity",
                    sql_text(&record.team_member_rns_identity),
                ),
                ("skill_uid", sql_text(&record.skill_uid)),
            ]
        },
        |record| {
            vec![
                (
                    "team_member_rns_identity",
                    sql_text(&record.team_member_rns_identity),
                ),
                ("skill_uid", sql_text(&record.skill_uid)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_task_skill_requirements",
        &before.task_skill_requirements,
        &after.task_skill_requirements,
        |record| {
            vec![
                ("task_uid", sql_text(&record.task_uid)),
                ("skill_uid", sql_text(&record.skill_uid)),
            ]
        },
        |record| {
            vec![
                ("task_uid", sql_text(&record.task_uid)),
                ("skill_uid", sql_text(&record.skill_uid)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_assignments",
        &before.assignments,
        &after.assignments,
        |record| vec![("assignment_uid", sql_text(&record.assignment_uid))],
        |record| {
            vec![
                ("assignment_uid", sql_text(&record.assignment_uid)),
                ("mission_uid", sql_text(&record.mission_uid)),
                ("task_uid", sql_text(&record.task_uid)),
                (
                    "team_member_rns_identity",
                    sql_text(&record.team_member_rns_identity),
                ),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_assignment_asset_links",
        &before.assignment_asset_links,
        &after.assignment_asset_links,
        |record| {
            vec![
                ("assignment_uid", sql_text(&record.assignment_uid)),
                ("asset_uid", sql_text(&record.asset_uid)),
            ]
        },
        |record| {
            vec![
                ("assignment_uid", sql_text(&record.assignment_uid)),
                ("asset_uid", sql_text(&record.asset_uid)),
            ]
        },
    )?;
    Ok(())
}

fn save_authorization_delta_tables(
    transaction: &Transaction<'_>,
    before: &RchCoreSnapshot,
    after: &RchCoreSnapshot,
) -> Result<(), RchCoreError> {
    save_payload_delta(
        transaction,
        "rch_identity_capabilities",
        &before.identity_capabilities,
        &after.identity_capabilities,
        |record| {
            vec![
                ("identity", sql_text(&record.identity)),
                ("capability", sql_text(&record.capability)),
            ]
        },
        |record| {
            vec![
                ("identity", sql_text(&record.identity)),
                ("capability", sql_text(&record.capability)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_mission_access_assignments",
        &before.mission_access_assignments,
        &after.mission_access_assignments,
        |record| {
            vec![
                ("mission_uid", sql_text(&record.mission_uid)),
                ("subject_type", sql_text(&record.subject_type)),
                ("subject_id", sql_text(&record.subject_id)),
            ]
        },
        |record| {
            vec![
                ("mission_uid", sql_text(&record.mission_uid)),
                ("subject_type", sql_text(&record.subject_type)),
                ("subject_id", sql_text(&record.subject_id)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_subject_operation_rights",
        &before.subject_operation_rights,
        &after.subject_operation_rights,
        |record| {
            vec![
                ("subject_type", sql_text(&record.subject_type)),
                ("subject_id", sql_text(&record.subject_id)),
                ("operation", sql_text(&record.operation)),
                ("scope_type", sql_text(&record.scope_type)),
                ("scope_id", sql_text(&record.scope_id)),
            ]
        },
        |record| {
            vec![
                ("subject_type", sql_text(&record.subject_type)),
                ("subject_id", sql_text(&record.subject_id)),
                ("operation", sql_text(&record.operation)),
                ("scope_type", sql_text(&record.scope_type)),
                ("scope_id", sql_text(&record.scope_id)),
            ]
        },
    )?;
    Ok(())
}

fn save_checklist_delta_tables(
    transaction: &Transaction<'_>,
    before: &RchCoreSnapshot,
    after: &RchCoreSnapshot,
) -> Result<(), RchCoreError> {
    save_payload_delta(
        transaction,
        "rch_checklists",
        &before.checklists,
        &after.checklists,
        |record| vec![("uid", sql_text(&record.uid))],
        |record| vec![("uid", sql_text(&record.uid))],
    )?;
    save_payload_delta(
        transaction,
        "rch_checklist_templates",
        &before.checklist_templates,
        &after.checklist_templates,
        |record| vec![("uid", sql_text(&record.uid))],
        |record| vec![("uid", sql_text(&record.uid))],
    )?;
    save_payload_delta(
        transaction,
        "rch_checklist_columns",
        &before.checklist_columns,
        &after.checklist_columns,
        |record| vec![("column_uid", sql_text(&record.column_uid))],
        |record| {
            vec![
                ("column_uid", sql_text(&record.column_uid)),
                (
                    "checklist_uid",
                    sql_optional_text(record.checklist_uid.as_deref()),
                ),
                (
                    "template_uid",
                    sql_optional_text(record.template_uid.as_deref()),
                ),
                ("display_order", sql_integer(record.display_order)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_checklist_tasks",
        &before.checklist_tasks,
        &after.checklist_tasks,
        |record| vec![("task_uid", sql_text(&record.task_uid))],
        |record| {
            vec![
                ("task_uid", sql_text(&record.task_uid)),
                ("checklist_uid", sql_text(&record.checklist_uid)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_checklist_cells",
        &before.checklist_cells,
        &after.checklist_cells,
        |record| vec![("cell_uid", sql_text(&record.cell_uid))],
        |record| {
            vec![
                ("cell_uid", sql_text(&record.cell_uid)),
                ("task_uid", sql_text(&record.task_uid)),
                ("column_uid", sql_text(&record.column_uid)),
            ]
        },
    )?;
    save_payload_delta(
        transaction,
        "rch_checklist_feed_publications",
        &before.checklist_feed_publications,
        &after.checklist_feed_publications,
        |record| vec![("publication_uid", sql_text(&record.publication_uid))],
        |record| {
            vec![
                ("publication_uid", sql_text(&record.publication_uid)),
                ("checklist_uid", sql_text(&record.checklist_uid)),
                ("mission_feed_uid", sql_text(&record.mission_feed_uid)),
                ("published_ts_ms", sql_integer(record.published_ts_ms)),
            ]
        },
    )?;
    save_skill_assignment_delta_tables(transaction, before, after)?;
    save_payload_delta(
        transaction,
        "rch_mission_changes",
        &before.mission_changes,
        &after.mission_changes,
        |record| vec![("uid", sql_text(&record.uid))],
        |record| {
            vec![
                ("uid", sql_text(&record.uid)),
                ("mission_uid", sql_text(&record.mission_uid)),
            ]
        },
    )?;
    save_audit_event_delta_tables(transaction, before, after)?;
    save_payload_delta(
        transaction,
        "rch_command_results",
        &before.command_results,
        &after.command_results,
        |record| vec![("command_id", sql_text(&record.command_id))],
        |record| vec![("command_id", sql_text(&record.command_id))],
    )?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn save_registry_snapshot_tables(
    transaction: &Transaction<'_>,
    snapshot: &RchCoreSnapshot,
) -> Result<(), RchCoreError> {
    for marker in &snapshot.markers {
        transaction.execute(
            "INSERT INTO rch_markers (object_destination_hash, payload) VALUES (?1, ?2)",
            params![marker.object_destination_hash, encode_msgpack(marker)?],
        )?;
    }
    for zone in &snapshot.zones {
        transaction.execute(
            "INSERT INTO rch_zones (zone_id, payload) VALUES (?1, ?2)",
            params![zone.zone_id, encode_msgpack(zone)?],
        )?;
    }
    for mission in &snapshot.missions {
        transaction.execute(
            "INSERT INTO rch_missions (uid, payload) VALUES (?1, ?2)",
            params![mission.uid, encode_msgpack(mission)?],
        )?;
    }
    for change in &snapshot.mission_changes {
        transaction.execute(
            "INSERT INTO rch_mission_changes (uid, mission_uid, payload) VALUES (?1, ?2, ?3)",
            params![change.uid, change.mission_uid, encode_msgpack(change)?],
        )?;
    }
    for entry in &snapshot.log_entries {
        transaction.execute(
            "INSERT INTO rch_log_entries (entry_uid, payload) VALUES (?1, ?2)",
            params![entry.entry_uid, encode_msgpack(entry)?],
        )?;
    }
    for attachment in &snapshot.file_attachments {
        transaction.execute(
            "INSERT INTO rch_file_attachments (file_id, category, payload)
             VALUES (?1, ?2, ?3)",
            params![
                attachment.file_id,
                attachment.category,
                encode_msgpack(attachment)?
            ],
        )?;
    }
    for eam in &snapshot.eam_snapshots {
        transaction.execute(
            "INSERT INTO rch_eam_snapshots (eam_uid, callsign, team_member_uid, team_uid, deleted_ts_ms, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                eam.eam_uid,
                eam.callsign,
                eam.team_member_uid,
                eam.team_uid,
                eam.deleted_ts_ms,
                encode_msgpack(eam)?
            ],
        )?;
    }
    for team in &snapshot.teams {
        transaction.execute(
            "INSERT INTO rch_teams (uid, payload) VALUES (?1, ?2)",
            params![team.uid, encode_msgpack(team)?],
        )?;
    }
    for link in &snapshot.mission_team_links {
        transaction.execute(
            "INSERT INTO rch_mission_team_links (mission_uid, team_uid, payload)
             VALUES (?1, ?2, ?3)",
            params![link.mission_uid, link.team_uid, encode_msgpack(link)?],
        )?;
    }
    for link in &snapshot.mission_zone_links {
        transaction.execute(
            "INSERT INTO rch_mission_zone_links (mission_uid, zone_id, payload)
             VALUES (?1, ?2, ?3)",
            params![link.mission_uid, link.zone_id, encode_msgpack(link)?],
        )?;
    }
    for link in &snapshot.mission_marker_links {
        transaction.execute(
            "INSERT INTO rch_mission_marker_links (mission_uid, marker_id, payload)
             VALUES (?1, ?2, ?3)",
            params![link.mission_uid, link.marker_id, encode_msgpack(link)?],
        )?;
    }
    for member in &snapshot.team_members {
        transaction.execute(
            "INSERT INTO rch_team_members (uid, payload) VALUES (?1, ?2)",
            params![member.uid, encode_msgpack(member)?],
        )?;
    }
    for link in &snapshot.team_member_client_links {
        transaction.execute(
            "INSERT INTO rch_team_member_client_links (team_member_uid, client_identity, payload)
             VALUES (?1, ?2, ?3)",
            params![
                link.team_member_uid,
                link.client_identity,
                encode_msgpack(link)?
            ],
        )?;
    }
    for asset in &snapshot.assets {
        transaction.execute(
            "INSERT INTO rch_assets (asset_uid, payload) VALUES (?1, ?2)",
            params![asset.asset_uid, encode_msgpack(asset)?],
        )?;
    }
    save_skill_assignment_snapshot_tables(transaction, snapshot)?;
    Ok(())
}

fn save_skill_assignment_snapshot_tables(
    transaction: &Transaction<'_>,
    snapshot: &RchCoreSnapshot,
) -> Result<(), RchCoreError> {
    for skill in &snapshot.skills {
        transaction.execute(
            "INSERT INTO rch_skills (skill_uid, payload) VALUES (?1, ?2)",
            params![skill.skill_uid, encode_msgpack(skill)?],
        )?;
    }
    for record in &snapshot.team_member_skills {
        transaction.execute(
            "INSERT INTO rch_team_member_skills (team_member_rns_identity, skill_uid, payload)
             VALUES (?1, ?2, ?3)",
            params![
                record.team_member_rns_identity,
                record.skill_uid,
                encode_msgpack(record)?
            ],
        )?;
    }
    for record in &snapshot.task_skill_requirements {
        transaction.execute(
            "INSERT INTO rch_task_skill_requirements (task_uid, skill_uid, payload)
             VALUES (?1, ?2, ?3)",
            params![record.task_uid, record.skill_uid, encode_msgpack(record)?],
        )?;
    }
    for assignment in &snapshot.assignments {
        transaction.execute(
            "INSERT INTO rch_assignments
                (assignment_uid, mission_uid, task_uid, team_member_rns_identity, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                assignment.assignment_uid,
                assignment.mission_uid,
                assignment.task_uid,
                assignment.team_member_rns_identity,
                encode_msgpack(assignment)?
            ],
        )?;
    }
    for link in &snapshot.assignment_asset_links {
        transaction.execute(
            "INSERT INTO rch_assignment_asset_links (assignment_uid, asset_uid, payload)
             VALUES (?1, ?2, ?3)",
            params![link.assignment_uid, link.asset_uid, encode_msgpack(link)?],
        )?;
    }
    save_checklist_snapshot_tables(transaction, snapshot)?;
    Ok(())
}

fn save_checklist_snapshot_tables(
    transaction: &Transaction<'_>,
    snapshot: &RchCoreSnapshot,
) -> Result<(), RchCoreError> {
    for checklist in &snapshot.checklists {
        transaction.execute(
            "INSERT INTO rch_checklists (uid, payload) VALUES (?1, ?2)",
            params![checklist.uid, encode_msgpack(checklist)?],
        )?;
    }
    for template in &snapshot.checklist_templates {
        transaction.execute(
            "INSERT INTO rch_checklist_templates (uid, payload) VALUES (?1, ?2)",
            params![template.uid, encode_msgpack(template)?],
        )?;
    }
    for column in &snapshot.checklist_columns {
        transaction.execute(
            "INSERT INTO rch_checklist_columns
                (column_uid, checklist_uid, template_uid, display_order, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                column.column_uid,
                column.checklist_uid,
                column.template_uid,
                column.display_order,
                encode_msgpack(column)?
            ],
        )?;
    }
    for task in &snapshot.checklist_tasks {
        transaction.execute(
            "INSERT INTO rch_checklist_tasks (task_uid, checklist_uid, payload)
             VALUES (?1, ?2, ?3)",
            params![task.task_uid, task.checklist_uid, encode_msgpack(task)?],
        )?;
    }
    for cell in &snapshot.checklist_cells {
        transaction.execute(
            "INSERT INTO rch_checklist_cells (cell_uid, task_uid, column_uid, payload)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                cell.cell_uid,
                cell.task_uid,
                cell.column_uid,
                encode_msgpack(cell)?
            ],
        )?;
    }
    for publication in &snapshot.checklist_feed_publications {
        transaction.execute(
            "INSERT INTO rch_checklist_feed_publications (publication_uid, checklist_uid, mission_feed_uid, published_ts_ms, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                publication.publication_uid,
                publication.checklist_uid,
                publication.mission_feed_uid,
                publication.published_ts_ms,
                encode_msgpack(publication)?
            ],
        )?;
    }
    Ok(())
}

fn value_as_str(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn required_text(args: &Value, keys: &[&str]) -> Result<String, RchCoreError> {
    optional_text(args, keys)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| RchCoreError::InvalidPayload(format!("{} is required", keys[0])))
}

fn required_non_empty(value: &str, field_name: &str) -> Result<String, RchCoreError> {
    let value = value.trim();
    if value.is_empty() {
        Err(RchCoreError::InvalidPayload(format!(
            "{field_name} is required"
        )))
    } else {
        Ok(value.to_string())
    }
}

fn normalize_roster_nickname(value: &str) -> Result<String, RchCoreError> {
    let value = required_non_empty(value, "nickname")?;
    if value.chars().count() > 32 {
        return Err(RchCoreError::InvalidPayload(
            "nickname cannot exceed 32 characters".to_string(),
        ));
    }
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        return Err(RchCoreError::InvalidPayload(
            "nickname may contain only letters, numbers, underscore, dash, or dot".to_string(),
        ));
    }
    Ok(value)
}

fn required_f64(args: &Value, key: &str) -> Result<f64, RchCoreError> {
    let object = args
        .as_object()
        .ok_or_else(|| RchCoreError::InvalidPayload(format!("{key} is required")))?;
    object.get(key).and_then(value_as_f64).ok_or_else(|| {
        RchCoreError::InvalidPayload(format!("{key} is required and must be numeric"))
    })
}

fn required_zone_points(args: &Value) -> Result<Vec<ZonePointRecord>, RchCoreError> {
    let points = args
        .get("points")
        .and_then(Value::as_array)
        .ok_or_else(|| RchCoreError::InvalidPayload("points must be a list".to_string()))?;
    let mut resolved = points
        .iter()
        .map(|point| {
            let lat = point.get("lat").and_then(value_as_f64).ok_or_else(|| {
                RchCoreError::InvalidPayload("zone point lat/lon must be numeric".to_string())
            })?;
            let lon = point.get("lon").and_then(value_as_f64).ok_or_else(|| {
                RchCoreError::InvalidPayload("zone point lat/lon must be numeric".to_string())
            })?;
            Ok::<ZonePointRecord, RchCoreError>(ZonePointRecord { lat, lon })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if resolved
        .first()
        .zip(resolved.last())
        .is_some_and(|(first, last)| zone_points_equal(first, last))
    {
        resolved.pop();
    }
    validate_zone_points(&resolved)?;
    Ok(resolved)
}

const MIN_ZONE_POINTS: usize = 3;
const MAX_ZONE_POINTS: usize = 200;
const COORD_EPSILON: f64 = 1e-9;

fn validate_zone_points(points: &[ZonePointRecord]) -> Result<(), RchCoreError> {
    if points.is_empty() {
        return Err(RchCoreError::InvalidPayload(
            "Zone points are required".to_string(),
        ));
    }
    if points.len() < MIN_ZONE_POINTS {
        return Err(RchCoreError::InvalidPayload(format!(
            "Zone must contain at least {MIN_ZONE_POINTS} points"
        )));
    }
    if points.len() > MAX_ZONE_POINTS {
        return Err(RchCoreError::InvalidPayload(format!(
            "Zone cannot contain more than {MAX_ZONE_POINTS} points"
        )));
    }
    for point in points {
        if !point.lat.is_finite() || !point.lon.is_finite() {
            return Err(RchCoreError::InvalidPayload(
                "zone point lat/lon must be numeric".to_string(),
            ));
        }
        if point.lat < -90.0 || point.lat > 90.0 {
            return Err(RchCoreError::InvalidPayload(
                "Zone point latitude must be between -90 and 90".to_string(),
            ));
        }
        if point.lon < -180.0 || point.lon > 180.0 {
            return Err(RchCoreError::InvalidPayload(
                "Zone point longitude must be between -180 and 180".to_string(),
            ));
        }
    }
    if zone_is_self_intersecting(points) {
        return Err(RchCoreError::InvalidPayload(
            "Zone polygon cannot self-intersect".to_string(),
        ));
    }
    Ok(())
}

fn zone_points_equal(left: &ZonePointRecord, right: &ZonePointRecord) -> bool {
    (left.lat - right.lat).abs() <= COORD_EPSILON && (left.lon - right.lon).abs() <= COORD_EPSILON
}

fn zone_orientation(a: &ZonePointRecord, b: &ZonePointRecord, c: &ZonePointRecord) -> f64 {
    (b.lon - a.lon) * (c.lat - a.lat) - (b.lat - a.lat) * (c.lon - a.lon)
}

fn zone_on_segment(a: &ZonePointRecord, b: &ZonePointRecord, c: &ZonePointRecord) -> bool {
    b.lon >= a.lon.min(c.lon) - COORD_EPSILON
        && b.lon <= a.lon.max(c.lon) + COORD_EPSILON
        && b.lat >= a.lat.min(c.lat) - COORD_EPSILON
        && b.lat <= a.lat.max(c.lat) + COORD_EPSILON
}

fn zone_segments_intersect(
    a1: &ZonePointRecord,
    a2: &ZonePointRecord,
    b1: &ZonePointRecord,
    b2: &ZonePointRecord,
) -> bool {
    let o1 = zone_orientation(a1, a2, b1);
    let o2 = zone_orientation(a1, a2, b2);
    let o3 = zone_orientation(b1, b2, a1);
    let o4 = zone_orientation(b1, b2, a2);

    if ((o1 > COORD_EPSILON && o2 < -COORD_EPSILON) || (o1 < -COORD_EPSILON && o2 > COORD_EPSILON))
        && ((o3 > COORD_EPSILON && o4 < -COORD_EPSILON)
            || (o3 < -COORD_EPSILON && o4 > COORD_EPSILON))
    {
        return true;
    }

    if o1.abs() <= COORD_EPSILON && zone_on_segment(a1, b1, a2) {
        return true;
    }
    if o2.abs() <= COORD_EPSILON && zone_on_segment(a1, b2, a2) {
        return true;
    }
    if o3.abs() <= COORD_EPSILON && zone_on_segment(b1, a1, b2) {
        return true;
    }
    if o4.abs() <= COORD_EPSILON && zone_on_segment(b1, a2, b2) {
        return true;
    }
    false
}

fn zone_is_self_intersecting(points: &[ZonePointRecord]) -> bool {
    let edge_count = points.len();
    for i in 0..edge_count {
        let a1 = &points[i];
        let a2 = &points[(i + 1) % edge_count];
        for j in (i + 1)..edge_count {
            if i == j || (i + 1) % edge_count == j || i == (j + 1) % edge_count {
                continue;
            }
            let b1 = &points[j];
            let b2 = &points[(j + 1) % edge_count];
            if zone_segments_intersect(a1, a2, b1, b2) {
                return true;
            }
        }
    }
    false
}

fn optional_text(args: &Value, keys: &[&str]) -> Option<String> {
    let object = args.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_as_str))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn optional_text_or_empty(args: &Value, keys: &[&str]) -> Option<String> {
    let object = args.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_as_str))
        .map(|value| value.trim().to_string())
}

fn normalize_marker_symbol(symbol: &str) -> Option<String> {
    let raw = symbol.trim().to_lowercase();
    if raw.is_empty() {
        return None;
    }
    let normalized = raw
        .split('.')
        .filter_map(|part| {
            let segment = normalize_marker_symbol_segment(part);
            (!segment.is_empty()).then_some(segment)
        })
        .collect::<Vec<_>>()
        .join(".");
    if normalized.is_empty() {
        return None;
    }
    Some(
        match normalized.as_str() {
            "pin" | "location" => "marker",
            "car" | "truck" | "auto" | "automobile" => "vehicle",
            "uav" | "uas" => "drone",
            "wildlife" | "pet" => "animal",
            "radar" | "telemetry" | "vehicle-sensor" => "sensor",
            "cctv" => "camera",
            "flame" | "wildfire" => "fire",
            "water" => "flood",
            "human" | "operator" => "person",
            "community" | "group-community" | "team" => "group",
            "building" | "facility" => "infrastructure",
            "medical" | "hospital" => "medic",
            "alarm" | "warning" => "alert",
            "mission" | "assignment" => "task",
            _ => normalized.as_str(),
        }
        .to_string(),
    )
}

fn is_supported_marker_symbol(symbol: &str) -> bool {
    matches!(
        symbol,
        "marker"
            | "friendly"
            | "hostile"
            | "neutral"
            | "unknown"
            | "vehicle"
            | "drone"
            | "animal"
            | "sensor"
            | "radio"
            | "antenna"
            | "camera"
            | "fire"
            | "flood"
            | "person"
            | "group"
            | "infrastructure"
            | "medic"
            | "alert"
            | "task"
    )
}

fn normalize_marker_symbol_segment(value: &str) -> String {
    let mut normalized = String::new();
    let mut pending_separator = false;
    for character in value.trim().chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if pending_separator && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(character);
            pending_separator = false;
        } else {
            pending_separator = true;
        }
    }
    normalized
}

fn optional_value_text(args: &Value, key: &str) -> Option<String> {
    args.as_object()?
        .get(key)
        .and_then(value_as_str)
        .map(|value| value.trim().to_string())
}

fn none_if_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn optional_bool(args: &Value, key: &str) -> Option<bool> {
    let value = args.as_object()?.get(key)?;
    value.as_bool().or_else(|| {
        value
            .as_str()
            .map(|text| matches!(text, "true" | "1" | "yes"))
    })
}

fn optional_i64(args: &Value, key: &str) -> Result<Option<i64>, RchCoreError> {
    let Some(value) = args.as_object().and_then(|object| object.get(key)) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value_as_i64(value)
        .map(Some)
        .ok_or_else(|| RchCoreError::InvalidPayload(format!("{key} must be an integer")))
}

fn optional_f64(args: &Value, key: &str) -> Result<Option<f64>, RchCoreError> {
    let Some(value) = args.as_object().and_then(|object| object.get(key)) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value_as_f64(value)
        .map(Some)
        .ok_or_else(|| RchCoreError::InvalidPayload(format!("{key} must be numeric")))
}

fn optional_timestamp_ms(args: &Value, keys: &[&str]) -> Result<Option<i64>, RchCoreError> {
    let Some(value) = keys.iter().find_map(|key| args.as_object()?.get(*key)) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    if let Some(timestamp) = value_as_i64(value) {
        return Ok(Some(if timestamp < 10_000_000_000 {
            timestamp * 1000
        } else {
            timestamp
        }));
    }
    let Some(text) = value.as_str() else {
        return Err(RchCoreError::InvalidPayload(
            "timestamp must be RFC3339 or Unix time".to_string(),
        ));
    };
    DateTime::parse_from_rfc3339(text)
        .map(|timestamp| Some(timestamp.timestamp() * 1000))
        .map_err(|_| RchCoreError::InvalidPayload("timestamp must be RFC3339".to_string()))
}

fn string_list(value: Option<&Value>, field_name: &str) -> Result<Vec<String>, RchCoreError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                value_as_str(item)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        RchCoreError::InvalidPayload(format!("{field_name} must contain strings"))
                    })
            })
            .collect(),
        Value::String(text) => Ok(text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect()),
        Value::Bool(_) | Value::Number(_) | Value::Object(_) => Err(RchCoreError::InvalidPayload(
            format!("{field_name} must be a list of strings"),
        )),
    }
}

fn checklist_columns_from_args(value: Option<&Value>) -> Result<Vec<Value>, RchCoreError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => {
            if items.iter().all(Value::is_object) {
                Ok(items.clone())
            } else {
                Err(RchCoreError::InvalidPayload(
                    "columns must contain objects".to_string(),
                ))
            }
        }
        Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => Err(
            RchCoreError::InvalidPayload("columns must be a list".to_string()),
        ),
    }
}

fn default_checklist_columns() -> Vec<Value> {
    vec![
        json!({
            "column_name": "Due",
            "display_order": 1,
            "column_type": "RELATIVE_TIME",
            "column_editable": false,
            "is_removable": false,
            "system_key": "DUE_RELATIVE_DTG",
        }),
        json!({
            "column_name": "Task",
            "display_order": 2,
            "column_type": "SHORT_STRING",
            "column_editable": true,
            "is_removable": true,
        }),
    ]
}

fn defaulted_checklist_columns(columns: Vec<Value>) -> Vec<Value> {
    if columns.is_empty() {
        default_checklist_columns()
    } else {
        columns
    }
}

fn validate_checklist_columns(columns: &[Value]) -> Result<(), RchCoreError> {
    let columns = defaulted_checklist_columns(columns.to_vec());
    let due_columns: Vec<_> = columns
        .iter()
        .filter(|column| {
            optional_text(column, &["system_key"]).as_deref() == Some("DUE_RELATIVE_DTG")
        })
        .collect();
    if due_columns.len() != 1 {
        return Err(RchCoreError::InvalidPayload(
            "Exactly one DUE_RELATIVE_DTG system column is required".to_string(),
        ));
    }
    let due = due_columns[0];
    if optional_text(due, &["column_type"]).as_deref() != Some("RELATIVE_TIME") {
        return Err(RchCoreError::InvalidPayload(
            "DUE_RELATIVE_DTG column must be RELATIVE_TIME".to_string(),
        ));
    }
    if optional_bool(due, "is_removable").unwrap_or(true) {
        return Err(RchCoreError::InvalidPayload(
            "DUE_RELATIVE_DTG column cannot be removable".to_string(),
        ));
    }
    Ok(())
}

fn apply_optional_role_fields(
    args: &Value,
    mission: &mut MissionRecord,
) -> Result<(), RchCoreError> {
    if let Some(password_hash) = optional_text_or_empty(args, &["password_hash"]) {
        mission.password_hash = none_if_empty(password_hash);
    }
    if let Some(role) = optional_text_or_empty(args, &["default_role"]) {
        mission.default_role = none_if_empty(normalize_mission_role(&role, "default_role")?);
    }
    if let Some(role) = optional_text_or_empty(args, &["owner_role"]) {
        mission.owner_role = none_if_empty(normalize_mission_role(&role, "owner_role")?);
    }
    if let Some(role) = optional_text_or_empty(args, &["mission_rde_role"]) {
        mission.mission_rde_role =
            none_if_empty(normalize_mission_role(&role, "mission_rde_role")?);
    }
    Ok(())
}

fn apply_optional_mission_status_fields(
    args: &Value,
    mission: &mut MissionRecord,
) -> Result<(), RchCoreError> {
    if args
        .as_object()
        .is_some_and(|object| object.contains_key("mission_status"))
    {
        mission.mission_status =
            normalize_mission_status(args.get("mission_status").and_then(value_as_str).as_deref())?;
    }
    if args
        .as_object()
        .is_some_and(|object| object.contains_key("mission_priority"))
    {
        mission.mission_priority = optional_i64(args, "mission_priority")?;
        if let Some(priority) = mission.mission_priority {
            if !(0..=100).contains(&priority) {
                return Err(RchCoreError::InvalidPayload(
                    "mission_priority must be between 0 and 100".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn apply_optional_member_contact_fields(
    args: &Value,
    member: &mut TeamMemberRecord,
) -> Result<(), RchCoreError> {
    if let Some(email) = optional_text_or_empty(args, &["email"]) {
        member.email = none_if_empty(email);
    }
    if let Some(phone) = optional_text_or_empty(args, &["phone"]) {
        member.phone = none_if_empty(phone);
    }
    if let Some(modulation) = optional_text_or_empty(args, &["modulation"]) {
        member.modulation = none_if_empty(modulation);
    }
    if let Some(availability) = optional_text_or_empty(args, &["availability"]) {
        member.availability = none_if_empty(availability);
    }
    if args
        .as_object()
        .is_some_and(|object| object.contains_key("certifications"))
    {
        member.certifications = string_list(args.get("certifications"), "certifications")?;
    }
    if let Some(last_active) = optional_text_or_empty(args, &["last_active"]) {
        member.last_active = none_if_empty(last_active);
    }
    Ok(())
}

fn team_refs_provided(args: &Value) -> bool {
    args.as_object().is_some_and(|object| {
        object.contains_key("mission_uid")
            || object.contains_key("mission_id")
            || object.contains_key("mission_uids")
    })
}

fn dedupe_non_empty(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if !value.is_empty() && seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    normalized
}

#[must_use]
pub fn rch_role_bundle_definitions() -> &'static [RchRoleBundleDefinition] {
    RCH_ROLE_BUNDLES
}

#[must_use]
pub fn rch_operation_definitions() -> Vec<&'static str> {
    let mut operations = Vec::new();
    operations.extend([
        "checklist.feed.publish",
        "checklist.join",
        "checklist.read",
        "checklist.template.delete",
        "checklist.template.read",
        "checklist.template.write",
        "checklist.upload",
        "checklist.write",
        "emergency.alert.send",
        "mission.audit.read",
        "mission.content.read",
        "mission.content.write",
        "mission.join",
        "mission.leave",
        "mission.message.send",
        "mission.registry.asset.read",
        "mission.registry.asset.write",
        "mission.registry.assignment.read",
        "mission.registry.assignment.write",
        "mission.registry.log.read",
        "mission.registry.log.write",
        "mission.registry.mission.read",
        "mission.registry.mission.write",
        "mission.registry.skill.read",
        "mission.registry.skill.write",
        "mission.registry.status.read",
        "mission.registry.status.write",
        "mission.registry.team.read",
        "mission.registry.team.write",
        "mission.zone.delete",
        "mission.zone.read",
        "mission.zone.write",
        "r3akt",
        "topic.create",
        "topic.delete",
        "topic.read",
        "topic.subscribe",
        "topic.write",
    ]);
    operations.extend([
        "admin.backup.write",
        "admin.config.write",
        "admin.enrollment.write",
        "admin.identity.revoke",
        "diagnostics.network.read",
        "runtime.delivery.read",
        "runtime.node.read",
        "runtime.routing.read",
    ]);
    for bundle in RCH_ROLE_BUNDLES {
        operations.extend(bundle.operations.iter().copied());
    }
    operations.sort_unstable();
    operations.dedup();
    operations
}

#[must_use]
pub fn rch_mission_role_bundle_definitions() -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut bundles = BTreeMap::new();
    bundles.insert(
        "MISSION_READONLY_SUBSCRIBER",
        MISSION_READONLY_OPERATION_LIST.to_vec(),
    );
    bundles.insert("MISSION_SUBSCRIBER", MISSION_WRITE_OPERATION_LIST.to_vec());
    bundles.insert("MISSION_OWNER", MISSION_OWNER_OPERATION_LIST.to_vec());
    for bundle in RCH_ROLE_BUNDLES {
        if bundle.scope_types.contains(&"mission") {
            bundles.insert(bundle.role, bundle.operations.to_vec());
        }
    }
    bundles
}

fn normalize_mission_role(value: &str, field_name: &str) -> Result<String, RchCoreError> {
    normalize_enum(
        value,
        field_name,
        &[
            "MISSION_OWNER",
            "MISSION_SUBSCRIBER",
            "MISSION_READONLY_SUBSCRIBER",
            ROLE_FIELD_OPERATOR,
            ROLE_TEAM_LEAD,
            ROLE_INCIDENT_COMMANDER,
            ROLE_LOGISTICS_RESOURCE_MANAGER,
            ROLE_COMMUNICATIONS_OPERATOR,
            ROLE_SYSTEM_ADMIN,
        ],
    )
}

fn mission_role_operations(role: &str) -> HashSet<&'static str> {
    match role {
        "MISSION_OWNER" => mission_owner_operations(),
        "MISSION_SUBSCRIBER" => mission_write_operations(),
        "MISSION_READONLY_SUBSCRIBER" => mission_readonly_operations(),
        ROLE_FIELD_OPERATOR => FIELD_OPERATOR_OPERATION_LIST.iter().copied().collect(),
        ROLE_TEAM_LEAD => TEAM_LEAD_OPERATION_LIST.iter().copied().collect(),
        ROLE_INCIDENT_COMMANDER => INCIDENT_COMMANDER_OPERATION_LIST.iter().copied().collect(),
        ROLE_LOGISTICS_RESOURCE_MANAGER => LOGISTICS_RESOURCE_MANAGER_OPERATION_LIST
            .iter()
            .copied()
            .collect(),
        ROLE_COMMUNICATIONS_OPERATOR => COMMUNICATIONS_OPERATOR_OPERATION_LIST
            .iter()
            .copied()
            .collect(),
        ROLE_SYSTEM_ADMIN => SYSTEM_ADMIN_OPERATION_LIST.iter().copied().collect(),
        _ => HashSet::new(),
    }
}

fn mission_readonly_operations() -> HashSet<&'static str> {
    MISSION_READONLY_OPERATION_LIST.iter().copied().collect()
}

fn mission_write_operations() -> HashSet<&'static str> {
    MISSION_WRITE_OPERATION_LIST.iter().copied().collect()
}

fn mission_owner_operations() -> HashSet<&'static str> {
    MISSION_OWNER_OPERATION_LIST.iter().copied().collect()
}

fn normalize_mission_status(value: Option<&str>) -> Result<String, RchCoreError> {
    normalize_enum(
        value.unwrap_or("MISSION_ACTIVE"),
        "mission_status",
        &[
            "MISSION_ACTIVE",
            "MISSION_PENDING",
            "MISSION_DELETED",
            "MISSION_COMPLETED_SUCCESS",
            "MISSION_COMPLETED_FAILED",
        ],
    )
}

fn normalize_team_color(value: &str) -> Result<String, RchCoreError> {
    normalize_enum(
        value,
        "color",
        &[
            "YELLOW",
            "RED",
            "BLUE",
            "ORANGE",
            "MAGENTA",
            "MAROON",
            "PURPLE",
            "DARK_BLUE",
            "CYAN",
            "TEAL",
            "GREEN",
            "DARK_GREEN",
            "BROWN",
        ],
    )
}

fn normalize_team_role(value: &str) -> Result<String, RchCoreError> {
    normalize_enum(
        value,
        "role",
        &[
            "TEAM_MEMBER",
            "TEAM_LEAD",
            ROLE_FIELD_OPERATOR,
            ROLE_INCIDENT_COMMANDER,
            ROLE_LOGISTICS_RESOURCE_MANAGER,
            ROLE_COMMUNICATIONS_OPERATOR,
            ROLE_SYSTEM_ADMIN,
            "HQ",
            "SNIPER",
            "MEDIC",
            "FORWARD_OBSERVER",
            "RTO",
            "K9",
        ],
    )
}

fn normalize_asset_status(value: &str) -> Result<String, RchCoreError> {
    normalize_enum(
        value,
        "status",
        &["AVAILABLE", "IN_USE", "LOST", "MAINTENANCE", "RETIRED"],
    )
}

fn normalize_task_status(value: &str) -> Result<String, RchCoreError> {
    normalize_enum(
        value,
        "status",
        &["PENDING", "COMPLETE", "COMPLETE_LATE", "LATE"],
    )
}

fn normalize_skill_level(value: i64, field_name: &str) -> Result<i64, RchCoreError> {
    if (0..=10).contains(&value) {
        Ok(value)
    } else {
        Err(RchCoreError::InvalidPayload(format!(
            "{field_name} must be between 0 and 10"
        )))
    }
}

fn normalize_checklist_mode(value: &str) -> Result<String, RchCoreError> {
    normalize_enum(value, "mode", &["ONLINE", "OFFLINE"])
}

fn normalize_checklist_sync_state(value: &str) -> Result<String, RchCoreError> {
    normalize_enum(
        value,
        "sync_state",
        &["LOCAL_ONLY", "UPLOAD_PENDING", "SYNCED"],
    )
}

fn normalize_checklist_origin(value: &str) -> Result<String, RchCoreError> {
    normalize_enum(
        value,
        "origin_type",
        &["BLANK_TEMPLATE", "RCH_TEMPLATE", "CSV_IMPORT"],
    )
}

fn normalize_checklist_column_type(value: &str) -> Result<String, RchCoreError> {
    normalize_enum(
        value,
        "column_type",
        &["SHORT_STRING", "LONG_STRING", "RELATIVE_TIME", "CHECKBOX"],
    )
}

fn normalize_checklist_user_status(value: &str) -> Result<String, RchCoreError> {
    normalize_enum(value, "user_status", &["PENDING", "COMPLETE"])
}

fn derive_task_status(
    user_status: &str,
    due_ts_ms: Option<i64>,
    completed_ts_ms: Option<i64>,
) -> (String, bool) {
    if user_status == "COMPLETE" {
        if let (Some(due), Some(completed)) = (due_ts_ms, completed_ts_ms) {
            if completed > due {
                return ("COMPLETE_LATE".to_string(), true);
            }
        }
        return ("COMPLETE".to_string(), false);
    }
    if due_ts_ms.is_some_and(|due| utc_now_ms() > due) {
        ("LATE".to_string(), true)
    } else {
        ("PENDING".to_string(), false)
    }
}

fn checklist_column_record_value(column: &ChecklistColumnRecord) -> Value {
    json!({
        "column_uid": column.column_uid,
        "column_name": column.column_name,
        "display_order": column.display_order,
        "column_type": column.column_type,
        "column_editable": column.column_editable,
        "background_color": column.background_color,
        "text_color": column.text_color,
        "is_removable": column.is_removable,
        "system_key": column.system_key,
    })
}

fn checklist_feed_publication_value(publication: &ChecklistFeedPublicationRecord) -> Value {
    json!({
        "publication_uid": publication.publication_uid,
        "checklist_uid": publication.checklist_uid,
        "mission_feed_uid": publication.mission_feed_uid,
        "published_at": millis_to_rfc3339(publication.published_ts_ms),
        "published_by_team_member_rns_identity": publication.published_by_team_member_rns_identity,
    })
}

struct EamStatuses {
    security: String,
    capability: String,
    preparedness: String,
    medical: String,
    mobility: String,
    comms: String,
}

impl EamStatuses {
    fn from_args(args: &Value) -> Result<Self, RchCoreError> {
        Ok(Self {
            security: normalize_eam_status(
                optional_value_text(args, "security_status").as_deref(),
            )?,
            capability: normalize_eam_status(
                optional_value_text(args, "capability_status").as_deref(),
            )?,
            preparedness: normalize_eam_status(
                optional_value_text(args, "preparedness_status").as_deref(),
            )?,
            medical: normalize_eam_status(optional_value_text(args, "medical_status").as_deref())?,
            mobility: normalize_eam_status(
                optional_value_text(args, "mobility_status").as_deref(),
            )?,
            comms: normalize_eam_status(optional_value_text(args, "comms_status").as_deref())?,
        })
    }
}

fn normalize_eam_status(value: Option<&str>) -> Result<String, RchCoreError> {
    match value
        .unwrap_or("Unknown")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "unknown" => Ok("Unknown".to_string()),
        "green" => Ok("Green".to_string()),
        "yellow" => Ok("Yellow".to_string()),
        "red" => Ok("Red".to_string()),
        _ => Err(RchCoreError::InvalidPayload(
            "status must be one of: Green, Red, Unknown, Yellow".to_string(),
        )),
    }
}

fn aggregate_eam_status(statuses: &EamStatuses) -> String {
    let values = [
        statuses.security.as_str(),
        statuses.capability.as_str(),
        statuses.preparedness.as_str(),
        statuses.medical.as_str(),
        statuses.mobility.as_str(),
        statuses.comms.as_str(),
    ];
    if values.contains(&"Red") {
        "Red".to_string()
    } else if values.contains(&"Yellow") {
        "Yellow".to_string()
    } else if values.iter().any(|value| *value != "Unknown") {
        "Green".to_string()
    } else {
        "Unknown".to_string()
    }
}

fn eam_is_expired(eam: &EamSnapshotRecord) -> bool {
    eam.ttl_seconds
        .is_some_and(|ttl| utc_now_ms() >= eam.reported_ts_ms + ttl * 1000)
}

fn reject_disallowed_eam_fields(args: &Value) -> Result<(), RchCoreError> {
    const DISALLOWED: &[(&str, &str)] = &[
        (
            "subject_type",
            "subject_type is not supported southbound; EAM is member-scoped only",
        ),
        (
            "subjectType",
            "subjectType is not supported southbound; EAM is member-scoped only",
        ),
        (
            "subject_id",
            "subject_id is not supported southbound; use team_member_uid",
        ),
        (
            "subjectId",
            "subjectId is not supported southbound; use team_member_uid",
        ),
        ("teamId", "teamId is not supported southbound; use team_uid"),
        (
            "reportedAt",
            "reportedAt is not supported southbound; use reported_at",
        ),
        (
            "reportedBy",
            "reportedBy is not supported southbound; use reported_by",
        ),
        (
            "overall_status",
            "overall_status is computed server-side and is not accepted on writes",
        ),
        (
            "overallStatus",
            "overallStatus is computed server-side and is not accepted on writes",
        ),
        (
            "groupName",
            "groupName is not supported southbound; use group_name",
        ),
        (
            "securityStatus",
            "securityStatus is not supported southbound; use security_status",
        ),
        (
            "capabilityStatus",
            "capabilityStatus is not supported southbound; use capability_status",
        ),
        (
            "preparednessStatus",
            "preparednessStatus is not supported southbound; use preparedness_status",
        ),
        (
            "medicalStatus",
            "medicalStatus is not supported southbound; use medical_status",
        ),
        (
            "mobilityStatus",
            "mobilityStatus is not supported southbound; use mobility_status",
        ),
        (
            "commsStatus",
            "commsStatus is not supported southbound; use comms_status",
        ),
        (
            "ttlSeconds",
            "ttlSeconds is not supported southbound; use ttl_seconds",
        ),
        (
            "securityCapability",
            "securityCapability is not supported southbound; use capability_status",
        ),
    ];
    if let Some(object) = args.as_object() {
        for (field, message) in DISALLOWED {
            if object.contains_key(*field) {
                return Err(RchCoreError::InvalidPayload((*message).to_string()));
            }
        }
    }
    Ok(())
}

fn canonical_team_color_for_uid(team_uid: &str) -> Option<&'static str> {
    canonical_team_for_uid(team_uid).map(|(_, color)| color)
}

fn canonical_team_for_uid(team_uid: &str) -> Option<(&'static str, &'static str)> {
    match team_uid {
        "d6b6e188b910d6bdd24d04b7a7ec5444" => Some(("d6b6e188b910d6bdd24d04b7a7ec5444", "YELLOW")),
        "65ce79a3a3e4b51ec0ec52d1d3d2b0b9" => Some(("65ce79a3a3e4b51ec0ec52d1d3d2b0b9", "RED")),
        "43341e5c822d99857fa6e8641f2ca9c0" => Some(("43341e5c822d99857fa6e8641f2ca9c0", "BLUE")),
        "a83eb640e4c4884be14831e3d7ef5ae0" => Some(("a83eb640e4c4884be14831e3d7ef5ae0", "ORANGE")),
        "7ac50a910f42b06cd9cb68dad3def681" => Some(("7ac50a910f42b06cd9cb68dad3def681", "MAGENTA")),
        "372824ef4f15881291455562f7570233" => Some(("372824ef4f15881291455562f7570233", "MAROON")),
        "4bf2a1d2217c8668942658137f2a6824" => Some(("4bf2a1d2217c8668942658137f2a6824", "PURPLE")),
        "cbb35fc9a8f5a91d7bd2b5e5b644edcd" => {
            Some(("cbb35fc9a8f5a91d7bd2b5e5b644edcd", "DARK_BLUE"))
        }
        "d4cd5030b68df059ec6beabe416dd6a6" => Some(("d4cd5030b68df059ec6beabe416dd6a6", "CYAN")),
        "4d7a7a974beec395bf83491604768499" => Some(("4d7a7a974beec395bf83491604768499", "TEAL")),
        "612a32262163b73a80eca944c2158546" => Some(("612a32262163b73a80eca944c2158546", "GREEN")),
        "341653613d4c76d56bee99c1f38177b1" => {
            Some(("341653613d4c76d56bee99c1f38177b1", "DARK_GREEN"))
        }
        "4efe72ac30f5b85142fdcab6d96c7631" => Some(("4efe72ac30f5b85142fdcab6d96c7631", "BROWN")),
        _ => None,
    }
}

fn canonical_team_uid_for_color(color: &str) -> Option<&'static str> {
    match color {
        "YELLOW" => Some("d6b6e188b910d6bdd24d04b7a7ec5444"),
        "RED" => Some("65ce79a3a3e4b51ec0ec52d1d3d2b0b9"),
        "BLUE" => Some("43341e5c822d99857fa6e8641f2ca9c0"),
        "ORANGE" => Some("a83eb640e4c4884be14831e3d7ef5ae0"),
        "MAGENTA" => Some("7ac50a910f42b06cd9cb68dad3def681"),
        "MAROON" => Some("372824ef4f15881291455562f7570233"),
        "PURPLE" => Some("4bf2a1d2217c8668942658137f2a6824"),
        "DARK_BLUE" => Some("cbb35fc9a8f5a91d7bd2b5e5b644edcd"),
        "CYAN" => Some("d4cd5030b68df059ec6beabe416dd6a6"),
        "TEAL" => Some("4d7a7a974beec395bf83491604768499"),
        "GREEN" => Some("612a32262163b73a80eca944c2158546"),
        "DARK_GREEN" => Some("341653613d4c76d56bee99c1f38177b1"),
        "BROWN" => Some("4efe72ac30f5b85142fdcab6d96c7631"),
        _ => None,
    }
}

fn canonical_team_from_team_args<'a>(
    args: &'a Value,
    requested_uid: Option<&'a str>,
) -> Option<(&'static str, &'static str)> {
    if let Some(uid) = requested_uid {
        return canonical_team_for_uid(uid);
    }
    let has_noncanonical_name = ["team_name", "name", "group_name"].iter().any(|field| {
        optional_text(args, &[*field])
            .and_then(|value| normalize_team_color(&value).ok())
            .is_none()
    });
    for field in ["color", "team_name", "name", "group_name"] {
        if field == "color" && has_noncanonical_name {
            continue;
        }
        let Some(value) = optional_text(args, &[field]) else {
            continue;
        };
        let Ok(color) = normalize_team_color(&value) else {
            continue;
        };
        if let Some(uid) = canonical_team_uid_for_color(&color) {
            return Some((uid, canonical_team_color_for_uid(uid)?));
        }
    }
    None
}

fn mission_expand_values(args: &Value) -> HashSet<String> {
    let Some(expand) = args.get("expand") else {
        return HashSet::new();
    };
    let raw_values = if let Some(text) = expand.as_str() {
        text.split(',').map(str::to_string).collect::<Vec<String>>()
    } else if let Some(values) = expand.as_array() {
        values
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect()
    } else {
        vec![expand.to_string()]
    };
    let mut tokens = HashSet::new();
    for raw in raw_values {
        let token = raw.trim().to_ascii_lowercase();
        if token.is_empty() {
            continue;
        }
        let mapped = match token.as_str() {
            "team" => "teams",
            "members" | "member" | "team_members" | "teammembers" => "team_members",
            "changes" | "change" => "mission_changes",
            "logs" | "log" | "entries" => "log_entries",
            "assignment" => "assignments",
            "checklist" => "checklists",
            "rde" => "mission_rde",
            other => other,
        };
        if mapped == "all" {
            tokens.extend(
                [
                    "topic",
                    "teams",
                    "team_members",
                    "assets",
                    "mission_changes",
                    "log_entries",
                    "assignments",
                    "checklists",
                    "mission_rde",
                ]
                .into_iter()
                .map(str::to_string),
            );
        } else if matches!(
            mapped,
            "topic"
                | "teams"
                | "team_members"
                | "assets"
                | "mission_changes"
                | "log_entries"
                | "assignments"
                | "checklists"
                | "mission_rde"
        ) {
            tokens.insert(mapped.to_string());
        }
    }
    tokens
}

fn normalize_csv_header(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_due_minutes(value: &str) -> Option<i64> {
    let text = value.trim().strip_prefix('+').unwrap_or(value.trim());
    if text.is_empty() {
        return None;
    }
    text.parse::<i64>().ok()
}

fn normalize_required_identity(
    value: Option<&str>,
    field_name: &str,
) -> Result<String, RchCoreError> {
    let text = value.unwrap_or_default().trim().to_ascii_lowercase();
    if text.is_empty() {
        Err(RchCoreError::InvalidPayload(format!(
            "{field_name} is required"
        )))
    } else {
        Ok(text)
    }
}

fn normalize_mission_change_type(value: Option<&str>) -> Result<String, RchCoreError> {
    normalize_enum(
        value.unwrap_or("ADD_CONTENT"),
        "change_type",
        &[
            "CREATE_MISSION",
            "DELETE_MISSION",
            "ADD_CONTENT",
            "REMOVE_CONTENT",
            "CREATE_DATA_FEED",
            "DELETE_DATA_FEED",
            "MAP_LAYER",
            "SITREP_IMPORTED",
        ],
    )
}

fn normalize_enum(
    value: &str,
    field_name: &str,
    allowed_values: &[&str],
) -> Result<String, RchCoreError> {
    let value = value.trim().to_ascii_uppercase();
    if value.is_empty() {
        return Ok(String::new());
    }
    if allowed_values.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(RchCoreError::InvalidPayload(format!(
            "{field_name} must be one of: {}",
            allowed_values.join(", ")
        )))
    }
}

fn utc_now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn utc_now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

fn millis_to_rfc3339(timestamp_ms: i64) -> String {
    DateTime::from_timestamp_millis(timestamp_ms).map_or_else(
        || "1970-01-01T00:00:00Z".to_string(),
        |timestamp| timestamp.to_rfc3339_opts(SecondsFormat::AutoSi, true),
    )
}

fn checklist_task_row_added_delta(checklist_payload: &Value, args: &Value) -> Option<Value> {
    let tasks = checklist_payload.get("tasks")?.as_array()?;
    let task_uid = optional_text(args, &["task_uid"]);
    let number = args.get("number").and_then(value_as_i64);
    let selected = if let Some(task_uid) = task_uid {
        tasks
            .iter()
            .find(|task| task.get("task_uid").and_then(value_as_str).as_deref() == Some(&task_uid))
    } else if let Some(number) = number {
        tasks
            .iter()
            .rev()
            .find(|task| task.get("number").and_then(value_as_i64) == Some(number))
    } else {
        tasks.last()
    }?;
    Some(json!({
        "op": "row_added",
        "mission_uid": checklist_payload.get("mission_id").cloned().unwrap_or(Value::Null),
        "checklist_uid": checklist_payload.get("uid").cloned().unwrap_or(Value::Null),
        "task_uid": selected.get("task_uid").cloned().unwrap_or(Value::Null),
        "number": selected.get("number").cloned().unwrap_or(Value::Null),
        "status": selected.get("task_status").cloned().unwrap_or(Value::Null),
        "user_status": selected.get("user_status").cloned().unwrap_or(Value::Null),
        "due_dtg": selected.get("due_dtg").cloned().unwrap_or(Value::Null),
        "due_relative_minutes": selected.get("due_relative_minutes").cloned().unwrap_or(Value::Null),
        "notes": selected.get("notes").cloned().unwrap_or(Value::Null),
        "legacy_value": selected.get("legacy_value").cloned().unwrap_or(Value::Null),
    }))
}

fn asset_mission_delta(asset: &AssetRecord, source_event_type: &str) -> Value {
    let op = match source_event_type {
        "mission.asset.deleted" => "delete",
        _ => "upsert",
    };
    json!({
        "op": op,
        "asset_uid": asset.asset_uid.clone(),
        "team_member_uid": asset.team_member_uid.clone(),
        "name": asset.name.clone(),
        "asset_type": asset.asset_type.clone(),
        "status": asset.status.clone(),
        "location": asset.location.clone(),
        "notes": asset.notes.clone(),
    })
}

fn assignment_mission_delta(
    core: &RchCore,
    assignment: &AssignmentRecord,
    op: &str,
    asset_uid: Option<String>,
) -> Value {
    let mut delta = json!({
        "op": op,
        "mission_uid": assignment.mission_uid.clone(),
        "task_uid": assignment.task_uid.clone(),
        "assignment_uid": assignment.assignment_uid.clone(),
        "team_member_rns_identity": assignment.team_member_rns_identity.clone(),
        "status": assignment.status.clone(),
        "due_dtg": assignment.due_dtg.clone(),
        "notes": assignment.notes.clone(),
        "assets": core.assignment_assets(&assignment.assignment_uid, &assignment.assets),
    });
    if let Some(asset_uid) = asset_uid {
        delta["asset_uid"] = json!(asset_uid);
    }
    delta
}

fn checklist_task_row_style_delta(checklist_payload: &Value, args: &Value) -> Option<Value> {
    let selected = checklist_task_from_payload(checklist_payload, args)?;
    Some(json!({
        "op": "row_style_set",
        "mission_uid": checklist_payload
            .get("mission_uid")
            .or_else(|| checklist_payload.get("mission_id"))
            .cloned()
            .unwrap_or(Value::Null),
        "checklist_uid": checklist_payload.get("uid").cloned().unwrap_or(Value::Null),
        "task_uid": selected.get("task_uid").cloned().unwrap_or(Value::Null),
        "number": selected.get("number").cloned().unwrap_or(Value::Null),
        "notes": selected.get("notes").cloned().unwrap_or(Value::Null),
        "legacy_value": selected.get("legacy_value").cloned().unwrap_or(Value::Null),
        "row_background_color": selected.get("row_background_color").cloned().unwrap_or(Value::Null),
        "line_break_enabled": selected.get("line_break_enabled").cloned().unwrap_or(Value::Null),
    }))
}

fn checklist_task_cell_set_delta(
    checklist_payload: &Value,
    args: &Value,
) -> (Option<Value>, Option<String>) {
    let Some(selected) = checklist_task_from_payload(checklist_payload, args) else {
        return (None, None);
    };
    let Some(column_uid) = optional_text(args, &["column_uid"]) else {
        return (None, None);
    };
    let cell = selected
        .get("cells")
        .and_then(Value::as_array)
        .and_then(|cells| {
            cells.iter().find(|cell| {
                cell.get("column_uid").and_then(value_as_str).as_deref()
                    == Some(column_uid.as_str())
            })
        });
    let updated_by = cell
        .and_then(|cell| {
            cell.get("updated_by_team_member_rns_identity")
                .and_then(value_as_str)
        })
        .and_then(none_if_empty);
    (
        Some(json!({
            "op": "cell_set",
            "mission_uid": checklist_payload
                .get("mission_uid")
                .or_else(|| checklist_payload.get("mission_id"))
                .cloned()
                .unwrap_or(Value::Null),
            "checklist_uid": checklist_payload.get("uid").cloned().unwrap_or(Value::Null),
            "task_uid": selected.get("task_uid").cloned().unwrap_or(Value::Null),
            "number": selected.get("number").cloned().unwrap_or(Value::Null),
            "notes": selected.get("notes").cloned().unwrap_or(Value::Null),
            "legacy_value": selected.get("legacy_value").cloned().unwrap_or(Value::Null),
            "column_uid": column_uid,
            "value": cell.and_then(|cell| cell.get("value")).cloned().unwrap_or(Value::Null),
            "updated_by_team_member_rns_identity": updated_by,
            "updated_at": cell.and_then(|cell| cell.get("updated_at")).cloned().unwrap_or(Value::Null),
        })),
        updated_by,
    )
}

fn checklist_task_status_set_delta(
    checklist_payload: &Value,
    args: &Value,
    previous_status: Option<&String>,
) -> (Option<Value>, Option<String>) {
    let Some(selected) = checklist_task_from_payload(checklist_payload, args) else {
        return (None, None);
    };
    let changed_by = optional_text_or_empty(args, &["changed_by_team_member_rns_identity"])
        .and_then(none_if_empty);
    (
        Some(json!({
            "op": "status_set",
            "mission_uid": checklist_payload
                .get("mission_uid")
                .or_else(|| checklist_payload.get("mission_id"))
                .cloned()
                .unwrap_or(Value::Null),
            "checklist_uid": checklist_payload.get("uid").cloned().unwrap_or(Value::Null),
            "task_uid": selected.get("task_uid").cloned().unwrap_or(Value::Null),
            "number": selected.get("number").cloned().unwrap_or(Value::Null),
            "notes": selected.get("notes").cloned().unwrap_or(Value::Null),
            "legacy_value": selected.get("legacy_value").cloned().unwrap_or(Value::Null),
            "previous_status": previous_status,
            "current_status": selected.get("task_status").cloned().unwrap_or(Value::Null),
            "user_status": selected.get("user_status").cloned().unwrap_or(Value::Null),
            "changed_by_team_member_rns_identity": changed_by,
            "changed_at": selected.get("updated_at").cloned().unwrap_or(Value::Null),
            "completed_at": selected.get("completed_at").cloned().unwrap_or(Value::Null),
            "due_dtg": selected.get("due_dtg").cloned().unwrap_or(Value::Null),
        })),
        changed_by,
    )
}

fn checklist_task_from_payload<'a>(
    checklist_payload: &'a Value,
    args: &Value,
) -> Option<&'a Value> {
    let tasks = checklist_payload.get("tasks")?.as_array()?;
    let task_uid = optional_text(args, &["task_uid"])?;
    tasks
        .iter()
        .find(|task| task.get("task_uid").and_then(value_as_str).as_deref() == Some(&task_uid))
}

fn append_mecp_keywords(keywords: &mut Vec<String>, content: &str) {
    let Ok(decoded) = r3akt_profile_rch::decode_mecp_message(content) else {
        return;
    };
    if !decoded.valid {
        return;
    }
    if let Some(category) = decoded.category {
        push_unique_keyword(keywords, format!("r3akt:event-type:{category}"));
    }
    for code in decoded.codes {
        push_unique_keyword(keywords, format!("r3akt:event-code:{code}"));
    }
}

fn push_unique_keyword(keywords: &mut Vec<String>, keyword: String) {
    if !keywords.iter().any(|existing| existing == &keyword) {
        keywords.push(keyword);
    }
}

fn mecp_log_entry_value(content: &str) -> Value {
    let Ok(decoded) = r3akt_profile_rch::decode_mecp_message(content) else {
        return Value::Null;
    };
    if !decoded.valid {
        return Value::Null;
    }
    let category_label = decoded
        .category
        .as_deref()
        .map_or("MECP", r3akt_profile_rch::mecp_category_label);
    let severity = decoded.severity.unwrap_or_default();
    let mut value = serde_json::to_value(&decoded).unwrap_or(Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert("category_label".to_string(), json!(category_label));
        object.insert(
            "severity_label".to_string(),
            json!(r3akt_profile_rch::mecp_severity_label(severity)),
        );
        object.insert(
            "severity_status".to_string(),
            json!(r3akt_profile_rch::mecp_severity_status(severity)),
        );
    }
    value
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

#[cfg(test)]
mod tests {
    use r3akt_profile_rch::RchSource;

    use super::*;

    mod rem_team_directory;

    fn delivery_decision(method: &str, reason: &str) -> OutboundDeliveryDecision {
        OutboundDeliveryDecision {
            method: method.to_string(),
            reason: reason.to_string(),
        }
    }

    fn command(command_type: &str, args: Value) -> MissionCommandEnvelope {
        let suffix = args.to_string();
        MissionCommandEnvelope {
            command_id: format!("cmd-{command_type}-{suffix}"),
            source: RchSource {
                rns_identity: "ABCDEF".to_string(),
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
    fn rch_topic_and_hash_normalization_match_python_contract() {
        assert_eq!(
            normalize_topic_id(Some("018f053d-7dec-7000-8000-000000000001")),
            Some("018f053d7dec70008000000000000001".to_string())
        );
        assert_eq!(
            normalize_topic_id(Some(" /Ops//Team ")),
            Some("/Ops//Team".to_string())
        );
        assert_eq!(normalize_hash(Some(" ABCDEF ")), Some("abcdef".to_string()));
        assert_eq!(
            normalize_topic_id_bytes(b" hello "),
            Some("hello".to_string())
        );
        assert_eq!(
            normalize_topic_id_bytes(&[0xff, 0x00]),
            Some("ff00".to_string())
        );
        assert_eq!(normalize_topic_id_bytes(b""), None);
        assert_eq!(
            normalize_hash_bytes(&[0xab, 0xcd]),
            Some("abcd".to_string())
        );
        assert_eq!(normalize_hash_bytes(b""), None);
    }

    #[test]
    fn core_save_preserves_identity_announces_written_by_row_api() {
        let mut store = RchSqliteStore::in_memory().expect("store");
        store
            .upsert_identity_announces(&[IdentityAnnounceRecord {
                destination_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                announced_identity_hash: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
                display_name: Some("REM Alpha".to_string()),
                source_interface: Some("test".to_string()),
                announce_capabilities: vec!["r3akt".to_string(), "emergencymessages".to_string()],
                client_type: "rem".to_string(),
                first_seen_ts_ms: 1,
                last_seen_ts_ms: 2,
            }])
            .expect("upsert announce");

        let core = RchCore::new();
        core.save_to_sqlite_preserving_identity_announces(&mut store)
            .expect("save core");

        assert_eq!(
            store
                .row_count("rch_identity_announces")
                .expect("announce count"),
            1
        );
    }

    #[test]
    fn wipe_database_removes_persisted_rows_and_sensitive_settings() {
        let mut store = RchSqliteStore::in_memory().expect("store");
        store
            .upsert_identity_announces(&[IdentityAnnounceRecord {
                destination_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                announced_identity_hash: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
                display_name: Some("REM Alpha".to_string()),
                source_interface: Some("test".to_string()),
                announce_capabilities: vec!["r3akt".to_string()],
                client_type: "rem".to_string(),
                first_seen_ts_ms: 1,
                last_seen_ts_ms: 2,
            }])
            .expect("upsert announce");
        store
            .set_setting_value("kill_switch_pin_hash", "sensitive")
            .expect("set sensitive setting");

        let preview = store.database_wipe_preview().expect("preview wipe");
        assert!(preview.rows_deleted >= 3);

        let report = store.wipe_database().expect("wipe database");
        assert!(report.rows_deleted >= 3);
        assert_eq!(
            store
                .row_count("rch_identity_announces")
                .expect("announce count"),
            0
        );
        assert_eq!(
            store
                .setting_value("kill_switch_pin_hash")
                .expect("setting lookup"),
            None
        );
        assert_eq!(store.schema_version().expect("schema version"), "3");
    }

    #[test]
    fn row_level_message_upsert_preserves_identity_announces() {
        let mut store = RchSqliteStore::in_memory().expect("store");
        store
            .upsert_identity_announces(&[IdentityAnnounceRecord {
                destination_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                announced_identity_hash: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
                display_name: Some("REM Alpha".to_string()),
                source_interface: Some("test".to_string()),
                announce_capabilities: vec!["r3akt".to_string(), "emergencymessages".to_string()],
                client_type: "rem".to_string(),
                first_seen_ts_ms: 1,
                last_seen_ts_ms: 2,
            }])
            .expect("upsert announce");

        store
            .upsert_message(&MessageRecord {
                message_id: "message-1".to_string(),
                topic_id: None,
                destination: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
                sender: "northbound".to_string(),
                content: "cmd".to_string(),
                delivery_mode: DeliveryMode::Targeted,
                delivery_method: "direct".to_string(),
                delivery_policy_reason: "rem_auto".to_string(),
                delivery_state: "queued".to_string(),
                delivery_metadata: json!({
                    "dispatch_status": "queued_deferred",
                    "next_attempt_at_ts_ms": 10,
                    "attempts": 0,
                }),
                created_ts_ms: 5,
                attachments: Vec::new(),
            })
            .expect("upsert message");

        assert_eq!(store.row_count("rch_messages").expect("message count"), 1);
        assert_eq!(
            store
                .row_count("rch_identity_announces")
                .expect("announce count"),
            1
        );
    }

    #[test]
    fn row_level_message_upsert_handles_ten_thousand_jobs() {
        let mut store = RchSqliteStore::in_memory().expect("store");
        store
            .upsert_identity_announces(&[IdentityAnnounceRecord {
                destination_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                announced_identity_hash: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
                display_name: Some("REM Alpha".to_string()),
                source_interface: Some("test".to_string()),
                announce_capabilities: vec!["r3akt".to_string(), "emergencymessages".to_string()],
                client_type: "rem".to_string(),
                first_seen_ts_ms: 1,
                last_seen_ts_ms: 2,
            }])
            .expect("upsert announce");

        for index in 0..10_000 {
            store
                .upsert_message(&MessageRecord {
                    message_id: format!("message-{index:05}"),
                    topic_id: None,
                    destination: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
                    sender: "northbound".to_string(),
                    content: "cmd".to_string(),
                    delivery_mode: DeliveryMode::Targeted,
                    delivery_method: "auto".to_string(),
                    delivery_policy_reason: "rem_auto".to_string(),
                    delivery_state: "queued".to_string(),
                    delivery_metadata: json!({
                        "dispatch_status": "queued_deferred",
                        "next_attempt_at_ts_ms": index,
                        "attempts": 0,
                        "batch_id": "batch-10k",
                    }),
                    created_ts_ms: index,
                    attachments: Vec::new(),
                })
                .expect("upsert message");
        }

        assert_eq!(
            store.row_count("rch_messages").expect("message count"),
            10_000
        );
        assert_eq!(
            store
                .row_count("rch_identity_announces")
                .expect("announce count"),
            1
        );
    }

    #[test]
    fn delivery_envelope_validates_required_fields_and_ttl() {
        let now = utc_now_ms();
        let envelope =
            build_delivery_envelope(BuildDeliveryEnvelope::new("ABCDEF")).expect("build");
        let validated = validate_delivery_envelope(&envelope.to_json(), now).expect("valid");

        assert_eq!(validated.sender, "abcdef");
        assert_eq!(validated.ttl_seconds, DEFAULT_TTL_SECONDS);

        let mut expired = envelope.to_json();
        expired["Born"] = json!(utc_now_ms() - 400_000);
        assert!(matches!(
            validate_delivery_envelope(&expired, utc_now_ms()),
            Err(RchCoreError::Delivery(reason)) if reason == "Message exceeded TTL"
        ));

        let mut future = envelope.to_json();
        future["Born"] = json!(now + (MAX_CLOCK_SKEW_SECONDS * 1000) + 1);
        assert!(matches!(
            validate_delivery_envelope(&future, now),
            Err(RchCoreError::Delivery(reason)) if reason == "Clock skew exceeds delivery budget"
        ));
    }

    #[test]
    fn python_rch_delivery_fixture_validates() {
        let payload = json!({
            "Born": 1_700_000_000_000_i64,
            "Content-Type": "text/plain; schema=lxmf.chat.v1",
            "Created-At": "2026-05-03T12:00:00Z",
            "Message-ID": "018f053d7dec70008000000000000001",
            "Priority": 3,
            "Schema-Version": "1",
            "Sender": "abcdef",
            "TTL": 300,
            "TopicID": "018f053d7dec70008000000000000002",
        });

        let envelope =
            validate_delivery_envelope(&payload, 1_700_000_000_000).expect("valid fixture");

        assert_eq!(envelope.message_id, "018f053d7dec70008000000000000001");
        assert_eq!(envelope.sender, "abcdef");
        assert_eq!(
            envelope.topic_id.as_deref(),
            Some("018f053d7dec70008000000000000002")
        );
        assert_eq!(envelope.priority, 3);
    }

    #[test]
    fn delivery_envelope_reapplies_rch_id_normalization_after_shared_validation() {
        let mut payload = json!({
            "Born": 1_700_000_000_000_i64,
            "Content-Type": "text/plain; schema=lxmf.chat.v1",
            "Message-ID": "018f053d-7dec-7000-8000-000000000001",
            "Priority": 3,
            "Schema-Version": "1",
            "Sender": "abcdef",
            "TTL": 300,
            "TopicID": "018f053d-7dec-7000-8000-000000000002",
        });

        let normalized =
            validate_delivery_envelope(&payload, 1_700_000_000_000).expect("valid envelope");
        assert_eq!(normalized.message_id, "018f053d7dec70008000000000000001");
        assert_eq!(
            normalized.topic_id.as_deref(),
            Some("018f053d7dec70008000000000000002")
        );

        payload["Message-ID"] = json!("  ");
        payload["TopicID"] = json!("  ");
        let generated =
            validate_delivery_envelope(&payload, 1_700_000_000_000).expect("valid blank IDs");
        assert_eq!(generated.message_id.len(), 32);
        assert!(
            generated
                .message_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        assert_eq!(generated.topic_id, None);
    }

    #[test]
    fn delivery_mode_rejects_mixed_topic_and_destination() {
        assert_eq!(
            classify_delivery_mode(Some("ops"), None).expect("fanout"),
            DeliveryMode::Fanout
        );
        assert!(matches!(
            classify_delivery_mode(Some("ops"), Some("abcd")),
            Err(RchCoreError::Delivery(_))
        ));
    }

    #[test]
    fn outbound_delivery_policy_matches_python_presence_and_cooldown_rules() {
        let identity = "11".repeat(16);
        let now = 1_700_000_000_000;
        let mut policy = OutboundDeliveryPolicy::default();

        assert_eq!(
            policy.delivery_decision(
                DeliveryMode::Targeted,
                Some(identity.as_str()),
                None,
                false,
                now
            ),
            delivery_decision("propagated", "no_fresh_presence")
        );

        policy.mark_presence(identity.as_str(), now - 1_000);
        assert_eq!(
            policy.delivery_decision(
                DeliveryMode::Targeted,
                Some(identity.as_str()),
                None,
                false,
                now
            ),
            delivery_decision("direct", "fresh_presence")
        );

        policy.mark_direct_failure(identity.as_str(), now);
        assert_eq!(
            policy.delivery_decision(
                DeliveryMode::Targeted,
                Some(identity.as_str()),
                Some(now - 1_000),
                false,
                now
            ),
            delivery_decision("propagated", "direct_cooldown")
        );

        assert_eq!(
            policy.delivery_decision(
                DeliveryMode::Targeted,
                Some(identity.as_str()),
                Some(now + 1_000),
                false,
                now + 1_000
            ),
            delivery_decision("direct", "fresh_presence")
        );
    }

    #[test]
    fn outbound_delivery_policy_propagates_fanout_broadcast_and_stale_presence() {
        let identity = "22".repeat(16);
        let now = 1_700_000_000_000;
        let mut policy = OutboundDeliveryPolicy::default();
        policy.mark_presence(
            identity.as_str(),
            now - RECENT_RUNTIME_PRESENCE_WINDOW_MS - 1,
        );

        assert_eq!(
            policy.delivery_decision(
                DeliveryMode::Fanout,
                Some(identity.as_str()),
                Some(now),
                true,
                now
            ),
            delivery_decision("propagated", "fanout_route")
        );
        assert_eq!(
            policy.delivery_decision(DeliveryMode::Broadcast, None, None, true, now),
            delivery_decision("propagated", "broadcast_route")
        );
        assert_eq!(
            policy.delivery_decision(
                DeliveryMode::Targeted,
                Some(identity.as_str()),
                Some(now - RECENT_ANNOUNCE_WINDOW_MS - 1),
                false,
                now
            ),
            delivery_decision("propagated", "no_fresh_presence")
        );
        assert_eq!(
            policy.delivery_decision(
                DeliveryMode::Targeted,
                Some(identity.as_str()),
                None,
                true,
                now
            ),
            delivery_decision("direct", "live_connection")
        );
    }

    #[test]
    fn create_subscribe_list_and_replay_are_cached() {
        let mut core = RchCore::new();
        let create = command(
            "topic.create",
            json!({ "topic_id": "mission-1", "topic_path": "mission-1", "topic_name": "Mission 1" }),
        );
        let first = core.handle_command(&create);
        let replay = core.handle_command(&create);

        assert_eq!(first.result.status, CommandResultStatus::Accepted);
        assert!(first.event.is_some());
        assert!(replay.event.is_none());

        let subscribe = command(
            "topic.subscribe",
            json!({ "topic_id": "mission-1", "destination": "FACEFEED" }),
        );
        let list = command("topic.list", json!({}));

        assert_eq!(
            core.handle_command(&subscribe).result.status,
            CommandResultStatus::Accepted
        );
        assert_eq!(core.subscribers("mission-1").len(), 1);
        core.validate_subscriber_invariants()
            .expect("subscriber invariants");
        assert_eq!(
            core.handle_command(&list).result.status,
            CommandResultStatus::Accepted
        );
    }

    #[test]
    fn topic_create_matches_python_name_path_and_id_rules() {
        let mut core = RchCore::new();

        let missing_name = core.handle_mission_sync_command(&command(
            "topic.create",
            json!({ "topic_path": "mission-no-name" }),
        ));
        assert_eq!(
            missing_name[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
        assert!(
            missing_name[1].results_field().expect("rejected")["reason"]
                .as_str()
                .expect("reason")
                .contains("topic_name and topic_path are required")
        );

        let created = core.handle_mission_sync_command(&command(
            "topic.create",
            json!({
                "topic_name": "Mission Generated",
                "topic_path": "mission/generated"
            }),
        ));
        let topic_id = created[1].results_field().expect("result")["result"]["TopicID"]
            .as_str()
            .expect("topic id");
        assert_eq!(topic_id.len(), 32);
        assert_ne!(topic_id, "mission.generated");
        assert_eq!(core.topics()[0].topic_name, "Mission Generated");
        assert_eq!(core.topics()[0].topic_path, "mission/generated");

        let explicit = core.handle_mission_sync_command(&command(
            "topic.create",
            json!({ "topic_id": "explicit.topic" }),
        ));
        assert_eq!(
            explicit[1].results_field().expect("result")["result"]["TopicID"],
            "explicit.topic"
        );
        let explicit_topic = core
            .topics()
            .into_iter()
            .find(|topic| topic.topic_id == "explicit.topic")
            .expect("explicit topic");
        assert_eq!(explicit_topic.topic_name, "explicit.topic");
        assert_eq!(explicit_topic.topic_path, "explicit.topic");
    }

    #[test]
    fn topic_list_preserves_python_created_order() {
        let mut core = RchCore::new();

        core.handle_mission_sync_command(&command(
            "topic.create",
            json!({
                "topic_id": "z-topic",
                "topic_path": "z-topic",
                "topic_name": "First"
            }),
        ));
        std::thread::sleep(std::time::Duration::from_millis(2));
        core.handle_mission_sync_command(&command(
            "topic.create",
            json!({
                "topic_id": "a-topic",
                "topic_path": "a-topic",
                "topic_name": "Second"
            }),
        ));

        let topics = core.topics();
        assert_eq!(topics[0].topic_id, "z-topic");
        assert_eq!(topics[1].topic_id, "a-topic");
    }

    #[test]
    fn topic_patch_and_delete_match_python_command_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "topic.create",
            json!({
                "topic_id": "mission-1",
                "topic_path": "mission-1",
                "topic_name": "Mission 1",
                "topic_description": "Original",
            }),
        ));
        core.handle_command(&command(
            "topic.subscribe",
            json!({ "topic_id": "mission-1", "destination": "FACEFEED" }),
        ));

        let patched = core.handle_mission_sync_command(&command(
            "topic.patch",
            json!({
                "topic_id": "mission-1",
                "topic_name": "Mission 1 Updated",
                "topic_description": "",
            }),
        ));

        assert_eq!(patched.len(), 2);
        assert_eq!(
            patched[1].results_field().expect("result")["result"]["topic_name"],
            "Mission 1 Updated"
        );
        assert_eq!(
            patched[1].event_field().expect("event")["event_type"],
            "mission.topic.updated"
        );
        assert_eq!(core.topics()[0].topic_description, "");

        let deleted = core.handle_mission_sync_command(&command(
            "topic.delete",
            json!({ "topic_id": "mission-1" }),
        ));

        assert_eq!(deleted.len(), 2);
        assert_eq!(
            deleted[1].results_field().expect("result")["result"]["topic_id"],
            "mission-1"
        );
        assert_eq!(
            deleted[1].event_field().expect("event")["event_type"],
            "mission.topic.deleted"
        );
        assert!(core.topics().is_empty());
        assert!(core.subscribers("mission-1").is_empty());
        core.validate_subscriber_invariants()
            .expect("subscriber invariants after topic deletion");
    }

    #[test]
    fn topic_subscriber_patch_and_delete_match_python_route_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "topic.create",
            json!({ "topic_id": "alerts", "topic_path": "alerts", "topic_name": "Alerts" }),
        ));
        core.handle_command(&command(
            "topic.subscribe",
            json!({
                "topic_id": "alerts",
                "destination": "dest-1",
                "reject_tests": 3,
                "metadata": { "role": "old" }
            }),
        ));

        let patched = core.handle_mission_sync_command(&command(
            "topic.subscriber.patch",
            json!({
                "subscriber_id": "dest-1",
                "destination": "dest-3",
                "topic_id": "alerts",
                "reject_tests": 0,
                "metadata": { "role": "new" }
            }),
        ));
        assert_eq!(
            patched[1].results_field().expect("result")["result"]["SubscriberID"],
            "dest-3"
        );
        assert_eq!(
            patched[1].results_field().expect("result")["result"]["Metadata"]["role"],
            "new"
        );
        assert_eq!(
            patched[1].results_field().expect("result")["result"]["RejectTests"],
            0
        );
        assert!(
            core.subscribers("alerts")
                .iter()
                .all(|item| item.node_id != "dest-1")
        );

        let deleted = core.handle_mission_sync_command(&command(
            "topic.subscriber.delete",
            json!({ "subscriber_id": "dest-3" }),
        ));
        assert_eq!(
            deleted[1].results_field().expect("result")["result"]["SubscriberID"],
            "dest-3"
        );
        assert!(core.subscribers("alerts").is_empty());

        let missing = core.handle_mission_sync_command(&command(
            "topic.subscriber.delete",
            json!({ "subscriber_id": "missing" }),
        ));
        assert_eq!(
            missing[1].results_field().expect("rejected")["reason_code"],
            "not_found"
        );
    }

    #[test]
    fn topic_patch_and_delete_use_python_rch_capabilities() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "topic.create",
            json!({ "topic_id": "mission-auth-topic", "topic_path": "mission-auth-topic", "topic_name": "Mission Auth Topic" }),
        ));
        core.set_authorization_required(true);

        let patch_rejected = core.handle_mission_sync_command(&command(
            "topic.patch",
            json!({ "topic_id": "mission-auth-topic", "topic_name": "Blocked" }),
        ));
        core.grant_identity_capability("abcdef", "topic.write");
        let patch_accepted = core.handle_mission_sync_command(&command(
            "topic.patch",
            json!({ "topic_id": "mission-auth-topic", "topic_name": "Allowed" }),
        ));
        let delete_rejected = core.handle_mission_sync_command(&command(
            "topic.delete",
            json!({ "topic_id": "mission-auth-topic" }),
        ));
        core.grant_identity_capability("abcdef", "topic.delete");
        let delete_accepted = core.handle_mission_sync_command(&command(
            "topic.delete",
            json!({ "topic_id": "mission-auth-topic" }),
        ));

        assert_eq!(
            patch_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "topic.write"
        );
        assert_eq!(
            patch_accepted[1].results_field().expect("result")["status"],
            "result"
        );
        assert_eq!(
            delete_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "topic.delete"
        );
        assert_eq!(
            delete_accepted[1].results_field().expect("result")["status"],
            "result"
        );
    }

    #[test]
    fn mission_join_leave_and_events_list_match_python_command_surface() {
        let mut core = RchCore::new();

        let joined = core.handle_mission_sync_command(&command("mission.join", json!({})));

        assert_eq!(joined.len(), 2);
        assert_eq!(
            joined[1].results_field().expect("result")["result"]["identity"],
            "abcdef"
        );
        assert_eq!(
            joined[1].results_field().expect("result")["result"]["joined"],
            true
        );
        assert_eq!(core.clients().len(), 1);

        let listed = core.handle_mission_sync_command(&command("mission.events.list", json!({})));

        assert_eq!(listed.len(), 2);
        let listed_event = &listed[1].results_field().expect("result")["result"]["events"][0];
        assert_eq!(listed_event["type"], "mission_command_processed");
        assert_eq!(listed_event["message"], "mission command processed");
        assert!(listed_event["timestamp"].as_str().is_some());
        assert_eq!(
            listed_event["metadata"]["command_id"],
            "cmd-mission.join-{}"
        );
        assert_eq!(listed_event["metadata"]["command_type"], "mission.join");
        assert_eq!(listed_event["metadata"]["identity"], "abcdef");
        assert_eq!(listed_event["metadata"]["event_type"], "mission.joined");
        assert_eq!(
            listed[1].event_field().expect("event")["event_type"],
            "mission.events.listed"
        );

        let left = core.handle_mission_sync_command(&command("mission.leave", json!({})));

        assert_eq!(left.len(), 2);
        assert_eq!(
            left[1].results_field().expect("result")["result"]["left"],
            true
        );
        assert!(core.clients().is_empty());
        assert_eq!(core.audit_events().len(), 3);
    }

    #[test]
    fn mission_roster_commands_persist_member_state() {
        let mut core = RchCore::new();

        core.handle_mission_sync_command(&command("mission.join", json!({})));
        core.handle_mission_sync_command(&command("mission.pause", json!({})));
        core.handle_mission_sync_command(&command(
            "mission.nick",
            json!({ "nickname": "Alpha_1" }),
        ));

        let paused = core.clients().pop().expect("client");
        assert_eq!(paused.identity, "ABCDEF");
        assert_eq!(paused.nickname.as_deref(), Some("Alpha_1"));
        assert_eq!(paused.role, "user");
        assert!(paused.paused);
        assert!(!paused.text_only);
        assert!(paused.last_chat_ts_ms.is_none());

        let mut store = RchSqliteStore::in_memory().expect("store");
        core.save_to_sqlite(&mut store).expect("save");
        let restored = RchCore::load_from_sqlite(&store)
            .expect("load")
            .expect("snapshot");
        let restored_client = restored.clients().pop().expect("restored client");
        assert_eq!(restored_client.nickname.as_deref(), Some("Alpha_1"));
        assert!(restored_client.paused);

        core.handle_mission_sync_command(&command("mission.resume", json!({})));
        assert!(!core.clients().pop().expect("resumed").paused);
    }

    #[test]
    fn mission_roster_rejects_banned_member_join() {
        let mut core = RchCore::new();
        core.set_identity_state("abcdef", true, false)
            .expect("ban identity");

        let joined = core.handle_mission_sync_command(&command("mission.join", json!({})));

        assert_eq!(
            joined[1].results_field().expect("rejected")["reason_code"],
            "forbidden"
        );
        assert!(core.clients().is_empty());
    }

    #[test]
    fn mission_events_list_uses_python_newest_first_limit() {
        let mut core = RchCore::new();

        for index in 0..55 {
            let result = core.handle_mission_sync_command(&command(
                "mission.join",
                json!({ "sequence": index }),
            ));
            assert_eq!(
                result[1].event_field().expect("event")["event_type"],
                "mission.joined"
            );
        }

        let listed = core.handle_mission_sync_command(&command("mission.events.list", json!({})));
        let events = listed[1].results_field().expect("result")["result"]["events"]
            .as_array()
            .expect("events");

        assert_eq!(events.len(), 50);
        assert_eq!(
            events[0]["metadata"]["command_id"],
            "cmd-mission.join-{\"sequence\":54}"
        );
        assert_eq!(
            events[49]["metadata"]["command_id"],
            "cmd-mission.join-{\"sequence\":5}"
        );
        assert!(
            !events.iter().any(|event| event["metadata"]["command_id"]
                == "cmd-mission.join-{\"sequence\":4}")
        );
    }

    #[test]
    fn mission_join_leave_and_events_use_python_rch_capabilities() {
        let mut core = RchCore::new();
        core.set_authorization_required(true);

        let join_rejected = core.handle_mission_sync_command(&command("mission.join", json!({})));
        core.grant_identity_capability("abcdef", "mission.join");
        let join_accepted = core.handle_mission_sync_command(&command("mission.join", json!({})));
        let events_rejected =
            core.handle_mission_sync_command(&command("mission.events.list", json!({})));
        core.grant_identity_capability("abcdef", "mission.audit.read");
        let events_accepted =
            core.handle_mission_sync_command(&command("mission.events.list", json!({})));
        let leave_rejected = core.handle_mission_sync_command(&command("mission.leave", json!({})));
        core.grant_identity_capability("abcdef", "mission.leave");
        let leave_accepted = core.handle_mission_sync_command(&command("mission.leave", json!({})));

        assert_eq!(
            join_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.join"
        );
        assert_eq!(
            join_accepted[1].results_field().expect("result")["status"],
            "result"
        );
        assert_eq!(
            events_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.audit.read"
        );
        assert_eq!(
            events_accepted[1].results_field().expect("result")["status"],
            "result"
        );
        assert_eq!(
            leave_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.leave"
        );
        assert_eq!(
            leave_accepted[1].results_field().expect("result")["status"],
            "result"
        );
    }

    #[test]
    fn marker_commands_create_list_and_patch_position() {
        let mut core = RchCore::new();

        let created = core.handle_mission_sync_command(&command(
            "mission.marker.create",
            json!({
                "name": "LZ Alpha",
                "marker_type": "marker",
                "symbol": "alert",
                "category": "aviation",
                "lat": 45.1,
                "lon": -63.2,
                "notes": "clear",
            }),
        ));

        assert_eq!(created.len(), 2);
        let marker_hash =
            created[1].results_field().expect("result")["result"]["object_destination_hash"]
                .as_str()
                .expect("marker hash")
                .to_string();
        let created_result = &created[1].results_field().expect("result")["result"];
        assert!(created_result["created_at"].as_str().is_some());
        assert!(created_result["updated_at"].as_str().is_some());
        assert_eq!(created_result["time"], created_result["updated_at"]);
        assert_eq!(created_result["stale_at"], created_result["updated_at"]);
        assert_eq!(
            created[1].event_field().expect("event")["event_type"],
            "mission.marker.created"
        );
        assert_eq!(core.markers().len(), 1);

        let listed = core.handle_mission_sync_command(&command("mission.marker.list", json!({})));
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["markers"][0]["name"],
            "LZ Alpha"
        );
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["markers"][0]["object_destination_hash"],
            marker_hash
        );

        let patched = core.handle_mission_sync_command(&command(
            "mission.marker.position.patch",
            json!({
                "object_destination_hash": marker_hash,
                "lat": 45.2,
                "lon": -63.3,
            }),
        ));
        assert_eq!(
            patched[1].results_field().expect("result")["result"]["position"]["lat"],
            45.2
        );
        assert_eq!(
            patched[1].event_field().expect("event")["event_type"],
            "mission.marker.position.updated"
        );

        let renamed = core.handle_mission_sync_command(&command(
            "mission.marker.patch",
            json!({
                "object_destination_hash": marker_hash,
                "name": "LZ Bravo",
            }),
        ));
        assert_eq!(
            renamed[1].results_field().expect("result")["result"]["name"],
            "LZ Bravo"
        );
        assert_eq!(
            renamed[1].event_field().expect("event")["event_type"],
            "mission.marker.updated"
        );

        let deleted = core.handle_mission_sync_command(&command(
            "mission.marker.delete",
            json!({ "object_destination_hash": marker_hash }),
        ));
        assert_eq!(
            deleted[1].results_field().expect("result")["result"]["object_destination_hash"],
            marker_hash
        );
        assert_eq!(
            deleted[1].event_field().expect("event")["event_type"],
            "mission.marker.deleted"
        );
        assert!(core.markers().is_empty());
    }

    #[test]
    fn marker_commands_normalize_python_rch_symbol_aliases() {
        let mut core = RchCore::new();

        let created = core.handle_mission_sync_command(&command(
            "mission.marker.create",
            json!({
                "name": "Community",
                "marker_type": "Group / Community",
                "symbol": "Group / Community",
                "category": "Group / Community",
                "lat": 12.0,
                "lon": 13.0,
            }),
        ));

        let result = &created[1].results_field().expect("result")["result"];
        assert_eq!(result["type"], "group");
        assert_eq!(result["symbol"], "group");
        assert_eq!(result["category"], "group");
    }

    #[test]
    fn marker_commands_reject_unsupported_type_and_symbol_like_python() {
        let mut core = RchCore::new();

        let unsupported_type = core.handle_mission_sync_command(&command(
            "mission.marker.create",
            json!({
                "marker_type": "not-in-registry",
                "symbol": "marker",
                "lat": 45.0,
                "lon": -63.0
            }),
        ));
        assert_eq!(
            unsupported_type[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
        assert!(
            unsupported_type[1].results_field().expect("rejected")["reason"]
                .as_str()
                .expect("reason")
                .contains("Unsupported marker type")
        );

        let unsupported_symbol = core.handle_mission_sync_command(&command(
            "mission.marker.create",
            json!({
                "marker_type": "marker",
                "symbol": "not-in-registry",
                "lat": 45.0,
                "lon": -63.0
            }),
        ));
        assert_eq!(
            unsupported_symbol[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
        assert!(
            unsupported_symbol[1].results_field().expect("rejected")["reason"]
                .as_str()
                .expect("reason")
                .contains("Unsupported marker symbol")
        );
        assert!(core.markers().is_empty());
    }

    #[test]
    fn zone_commands_create_list_patch_and_delete() {
        let mut core = RchCore::new();

        let created = core.handle_mission_sync_command(&command(
            "mission.zone.create",
            json!({
                "name": "Hot Zone",
                "points": [
                    { "lat": 45.0, "lon": -63.0 },
                    { "lat": 45.1, "lon": -63.1 },
                    { "lat": 45.1, "lon": -63.0 }
                ],
            }),
        ));

        let zone_id = created[1].results_field().expect("result")["result"]["zone_id"]
            .as_str()
            .expect("zone id")
            .to_string();
        let created_result = &created[1].results_field().expect("result")["result"];
        assert!(created_result["created_at"].as_str().is_some());
        assert!(created_result["updated_at"].as_str().is_some());
        assert_eq!(
            created_result["points"].as_array().expect("points").len(),
            3
        );
        assert_eq!(
            created[1].event_field().expect("event")["event_type"],
            "mission.zone.created"
        );

        let listed = core.handle_mission_sync_command(&command("mission.zone.list", json!({})));
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["zones"][0]["name"],
            "Hot Zone"
        );
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["zones"][0]["zone_id"],
            zone_id
        );

        let patched = core.handle_mission_sync_command(&command(
            "mission.zone.patch",
            json!({
                "zone_id": zone_id,
                "name": "Warm Zone",
                "points": [
                    { "lat": 46.0, "lon": -64.0 },
                    { "lat": 46.1, "lon": -64.1 },
                    { "lat": 46.1, "lon": -64.0 }
                ],
            }),
        ));
        assert_eq!(
            patched[1].results_field().expect("result")["result"]["points"][0]["lat"],
            46.0
        );

        let zone_id = patched[1].results_field().expect("result")["result"]["zone_id"]
            .as_str()
            .expect("zone id")
            .to_string();
        let deleted = core.handle_mission_sync_command(&command(
            "mission.zone.delete",
            json!({ "zone_id": zone_id }),
        ));

        assert_eq!(
            deleted[1].event_field().expect("event")["event_type"],
            "mission.zone.deleted"
        );
        assert!(core.zones().is_empty());
    }

    #[test]
    fn zone_commands_reject_invalid_geometry() {
        let mut core = RchCore::new();

        let too_few_points = core.handle_mission_sync_command(&command(
            "mission.zone.create",
            json!({
                "name": "Too Small",
                "points": [
                    { "lat": 45.0, "lon": -63.0 },
                    { "lat": 45.1, "lon": -63.1 }
                ],
            }),
        ));
        assert_eq!(
            too_few_points[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
        assert!(
            too_few_points[1].results_field().expect("rejected")["reason"]
                .as_str()
                .expect("reason")
                .contains("at least 3 points")
        );

        let self_intersecting = core.handle_mission_sync_command(&command(
            "mission.zone.create",
            json!({
                "name": "Bow Tie",
                "points": [
                    { "lat": 0.0, "lon": 0.0 },
                    { "lat": 1.0, "lon": 1.0 },
                    { "lat": 0.0, "lon": 1.0 },
                    { "lat": 1.0, "lon": 0.0 }
                ],
            }),
        ));
        assert_eq!(
            self_intersecting[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
        assert!(
            self_intersecting[1].results_field().expect("rejected")["reason"]
                .as_str()
                .expect("reason")
                .contains("self-intersect")
        );

        let created = core.handle_mission_sync_command(&command(
            "mission.zone.create",
            json!({
                "name": "Patch Target",
                "points": [
                    { "lat": 45.0, "lon": -63.0 },
                    { "lat": 45.1, "lon": -63.1 },
                    { "lat": 45.1, "lon": -63.0 }
                ],
            }),
        ));
        let zone_id = created[1].results_field().expect("result")["result"]["zone_id"]
            .as_str()
            .expect("zone id");

        let rejected_patch = core.handle_mission_sync_command(&command(
            "mission.zone.patch",
            json!({
                "zone_id": zone_id,
                "points": [
                    { "lat": 0.0, "lon": 0.0 },
                    { "lat": 1.0, "lon": 1.0 },
                    { "lat": 0.0, "lon": 1.0 },
                    { "lat": 1.0, "lon": 0.0 }
                ],
            }),
        ));
        assert_eq!(
            rejected_patch[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
    }

    #[test]
    fn marker_and_zone_commands_use_python_rch_capabilities() {
        let mut core = RchCore::new();
        core.set_authorization_required(true);

        let marker_list =
            core.handle_mission_sync_command(&command("mission.marker.list", json!({})));
        let marker_create = core.handle_mission_sync_command(&command(
            "mission.marker.create",
            json!({ "lat": 1.0, "lon": 2.0 }),
        ));
        let zone_list = core.handle_mission_sync_command(&command("mission.zone.list", json!({})));
        let zone_create = core.handle_mission_sync_command(&command(
            "mission.zone.create",
            json!({ "points": [{ "lat": 1.0, "lon": 2.0 }] }),
        ));
        let zone_delete =
            core.handle_mission_sync_command(&command("mission.zone.delete", json!({})));

        assert_eq!(
            marker_list[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.content.read"
        );
        assert_eq!(
            marker_create[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.content.write"
        );
        assert_eq!(
            zone_list[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.zone.read"
        );
        assert_eq!(
            zone_create[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.zone.write"
        );
        assert_eq!(
            zone_delete[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.zone.delete"
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn registry_mission_commands_match_python_crud_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "topic.create",
            json!({ "topic_id": "ops-main", "topic_path": "ops-main", "topic_name": "Ops Main" }),
        ));

        let created = core.handle_mission_sync_command(&command(
            "mission.registry.mission.upsert",
            json!({
                "uid": "mission-alpha",
                "mission_name": "Mission Alpha",
                "description": "Initial",
                "topic_id": "ops-main",
                "keywords": ["medical", "shelter"],
                "feeds": "blue,red",
                "default_role": "MISSION_SUBSCRIBER",
                "mission_status": "MISSION_PENDING",
                "mission_priority": 75,
                "invite_only": true,
            }),
        ));

        assert_eq!(created.len(), 2);
        assert_eq!(
            created[1].event_field().expect("event")["event_type"],
            "mission.registry.mission.upserted"
        );
        assert_eq!(
            created[1].results_field().expect("result")["result"]["uid"],
            "mission-alpha"
        );
        assert_eq!(
            created[1].results_field().expect("result")["result"]["mission_status"],
            "MISSION_PENDING"
        );
        assert_eq!(core.missions().len(), 1);

        let listed = core.handle_mission_sync_command(&command(
            "mission.registry.mission.list",
            json!({ "limit": 10 }),
        ));
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["missions"][0]["mission_name"],
            "Mission Alpha"
        );

        let patched = core.handle_mission_sync_command(&command(
            "mission.registry.mission.patch",
            json!({
                "mission_uid": "mission-alpha",
                "patch": {
                    "mission_name": "Mission Alpha Updated",
                    "mission_status": "MISSION_ACTIVE",
                    "mission_priority": 50
                }
            }),
        ));
        assert_eq!(
            patched[1].event_field().expect("event")["event_type"],
            "mission.registry.mission.updated"
        );
        assert_eq!(
            patched[1].results_field().expect("result")["result"]["mission_name"],
            "Mission Alpha Updated"
        );

        let retrieved = core.handle_mission_sync_command(&command(
            "mission.registry.mission.get",
            json!({ "mission_uid": "mission-alpha" }),
        ));
        assert_eq!(
            retrieved[1].event_field().expect("event")["event_type"],
            "mission.registry.mission.retrieved"
        );

        core.handle_command(&command(
            "mission.zone.create",
            json!({
                "name": "Zone Alpha",
                "points": [
                    { "lat": 45.0, "lon": -63.0 },
                    { "lat": 45.1, "lon": -63.1 },
                    { "lat": 45.2, "lon": -63.0 }
                ]
            }),
        ));
        let zone_id = core.zones()[0].zone_id.clone();
        let zone_link = core.handle_mission_sync_command(&command(
            "mission.registry.mission.zone.link",
            json!({ "mission_uid": "mission-alpha", "zone_id": zone_id.clone() }),
        ));
        assert_eq!(
            zone_link[1].event_field().expect("event")["event_type"],
            "mission.registry.mission.zone.linked"
        );
        assert_eq!(
            zone_link[1].results_field().expect("result")["result"]["zones"][0],
            zone_id
        );

        core.handle_mission_sync_command(&command(
            "mission.marker.create",
            json!({ "name": "Marker Alpha", "lat": 45.0, "lon": -63.0 }),
        ));
        let marker_id = core.markers()[0].object_destination_hash.clone();
        let marker_link = core.handle_mission_sync_command(&command(
            "mission.registry.mission.marker.link",
            json!({ "mission_uid": "mission-alpha", "marker_id": marker_id.clone() }),
        ));
        assert_eq!(
            marker_link[1].event_field().expect("event")["event_type"],
            "mission.registry.mission.marker.linked"
        );
        assert_eq!(
            marker_link[1].results_field().expect("result")["result"]["markers"][0],
            marker_id
        );

        let rde = core.handle_mission_sync_command(&command(
            "mission.registry.mission.rde.set",
            json!({ "mission_uid": "mission-alpha", "role": "MISSION_OWNER" }),
        ));
        assert_eq!(
            rde[1].event_field().expect("event")["event_type"],
            "mission.registry.mission.rde.updated"
        );
        assert_eq!(
            rde[1].results_field().expect("result")["result"]["role"],
            "MISSION_OWNER"
        );

        core.handle_mission_sync_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": "team-alpha", "team_name": "Team Alpha", "mission_uid": "mission-alpha" }),
        ));
        core.handle_mission_sync_command(&command(
            "mission.registry.team_member.upsert",
            json!({ "uid": "member-alpha", "team_uid": "team-alpha", "rns_identity": "peer-alpha" }),
        ));
        core.handle_mission_sync_command(&command(
            "mission.registry.asset.upsert",
            json!({ "asset_uid": "asset-alpha", "team_member_uid": "member-alpha", "name": "Radio" }),
        ));
        seed_checklist_with_task(&mut core);
        core.checklists
            .get_mut("checklist-1")
            .expect("checklist")
            .mission_uid = Some("mission-alpha".to_string());
        let assignment = core.handle_mission_sync_command(&command(
            "mission.registry.assignment.upsert",
            json!({
                "assignment_uid": "assignment-alpha",
                "mission_uid": "mission-alpha",
                "task_uid": "task-1",
                "team_member_rns_identity": "peer-alpha",
                "assets": ["asset-alpha"]
            }),
        ));
        let assignment_result = assignment[1].results_field().expect("result");
        assert_eq!(
            assignment_result["result"]["assignment_uid"], "assignment-alpha",
            "{assignment_result}"
        );
        core.handle_mission_sync_command(&command(
            "mission.registry.log_entry.upsert",
            json!({
                "entry_uid": "log-alpha",
                "mission_uid": "mission-alpha",
                "content": "Mission log",
                "team_member_rns_identity": "peer-alpha"
            }),
        ));
        core.handle_mission_sync_command(&command(
            "mission.registry.mission_change.upsert",
            json!({
                "uid": "change-alpha",
                "mission_uid": "mission-alpha",
                "name": "Manual change",
                "change_type": "ADD_CONTENT"
            }),
        ));
        let expanded = core.handle_mission_sync_command(&command(
            "mission.registry.mission.get",
            json!({ "mission_uid": "mission-alpha", "expand": "all" }),
        ));
        let expanded = &expanded[1].results_field().expect("result")["result"];
        assert_eq!(expanded["topic"]["topic_id"], "ops-main");
        assert_eq!(expanded["teams"][0]["uid"], "team-alpha");
        assert_eq!(expanded["team_members"][0]["uid"], "member-alpha");
        assert_eq!(expanded["assets"][0]["asset_uid"], "asset-alpha");
        assert_eq!(
            expanded["assignments"][0]["assignment_uid"],
            "assignment-alpha"
        );
        assert_eq!(expanded["checklists"][0]["uid"], "checklist-1");
        assert_eq!(expanded["checklists"][0]["tasks"][0]["task_uid"], "task-1");
        assert!(
            expanded["mission_changes"]
                .as_array()
                .expect("changes")
                .iter()
                .any(|change| change["uid"] == "change-alpha")
        );
        assert_eq!(expanded["log_entries"][0]["entry_uid"], "log-alpha");
        assert_eq!(expanded["mission_rde"]["role"], "MISSION_OWNER");

        let zone_unlink = core.handle_mission_sync_command(&command(
            "mission.registry.mission.zone.unlink",
            json!({ "mission_uid": "mission-alpha", "zone_id": zone_id }),
        ));
        assert_eq!(
            zone_unlink[1].results_field().expect("result")["result"]["zones"],
            json!([])
        );

        let marker_unlink = core.handle_mission_sync_command(&command(
            "mission.registry.mission.marker.unlink",
            json!({ "mission_uid": "mission-alpha", "marker_id": marker_id }),
        ));
        assert_eq!(
            marker_unlink[1].results_field().expect("result")["result"]["markers"],
            json!([])
        );

        let deleted = core.handle_mission_sync_command(&command(
            "mission.registry.mission.delete",
            json!({ "mission_uid": "mission-alpha" }),
        ));
        assert_eq!(
            deleted[1].results_field().expect("result")["result"]["mission_status"],
            "MISSION_DELETED"
        );
    }

    #[test]
    fn registry_mission_parent_cycle_rejects_after_acceptance() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "parent", "mission_name": "Parent" }),
        ));
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "child", "mission_name": "Child", "parent_uid": "parent" }),
        ));

        let rejected = core.handle_mission_sync_command(&command(
            "mission.registry.mission.parent.set",
            json!({ "mission_uid": "parent", "parent_uid": "child" }),
        ));

        assert_eq!(rejected.len(), 2);
        assert_eq!(
            rejected[0].results_field().expect("accepted")["status"],
            "accepted"
        );
        assert_eq!(
            rejected[1].results_field().expect("rejected")["status"],
            "rejected"
        );
        assert_eq!(
            rejected[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
    }

    #[test]
    fn registry_mission_commands_use_python_rch_capabilities() {
        let mut core = RchCore::new();
        core.set_authorization_required(true);

        let write_rejected = core.handle_mission_sync_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "auth-mission" }),
        ));
        core.grant_identity_capability("abcdef", "mission.registry.mission.write");
        let write_accepted = core.handle_mission_sync_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "auth-mission" }),
        ));
        let read_rejected = core.handle_mission_sync_command(&command(
            "mission.registry.mission.get",
            json!({ "mission_uid": "auth-mission" }),
        ));
        let zone_rejected = core.handle_mission_sync_command(&command(
            "mission.registry.mission.zone.link",
            json!({ "mission_uid": "auth-mission", "zone_id": "zone-auth" }),
        ));
        core.grant_identity_capability("abcdef", "mission.registry.mission.read");
        let read_accepted = core.handle_mission_sync_command(&command(
            "mission.registry.mission.get",
            json!({ "mission_uid": "auth-mission" }),
        ));

        assert_eq!(
            write_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.registry.mission.write"
        );
        assert_eq!(
            write_accepted[1].results_field().expect("result")["status"],
            "result"
        );
        assert_eq!(
            read_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.registry.mission.read"
        );
        assert_eq!(
            zone_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.zone.write"
        );
        assert_eq!(
            read_accepted[1].results_field().expect("result")["status"],
            "result"
        );
    }

    #[test]
    fn registry_mission_changes_and_log_entries_match_python_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-1", "mission_name": "Mission 1" }),
        ));

        let change_upsert = core.handle_mission_sync_command(&command(
            "mission.registry.mission_change.upsert",
            json!({
                "uid": "change-1",
                "mission_uid": "mission-1",
                "name": "Updated objective",
                "change_type": "ADD_CONTENT",
                "delta": { "field": "objective" },
            }),
        ));
        assert_eq!(
            change_upsert[1].results_field().expect("result")["result"]["uid"],
            "change-1"
        );
        assert_eq!(
            change_upsert[1].event_field().expect("event")["event_type"],
            "mission.registry.mission_change.upserted"
        );

        let change_list = core.handle_mission_sync_command(&command(
            "mission.registry.mission_change.list",
            json!({ "mission_uid": "mission-1" }),
        ));
        assert_eq!(
            change_list[1].results_field().expect("result")["result"]["mission_changes"][0]["uid"],
            "change-1"
        );

        let log_upsert = core.handle_mission_sync_command(&command(
            "mission.registry.log_entry.upsert",
            json!({
                "entry_uid": "log-1",
                "content": "Log event",
            }),
        ));
        assert_eq!(
            log_upsert[1].results_field().expect("result")["result"]["entry_uid"],
            "log-1"
        );
        assert_eq!(
            log_upsert[1].results_field().expect("result")["result"]["mission_uid"],
            "mission-default"
        );
        assert_eq!(
            log_upsert[1].results_field().expect("result")["result"]["callsign"],
            "Field Agent"
        );

        let log_list = core.handle_mission_sync_command(&command(
            "mission.registry.log_entry.list",
            json!({ "mission_uid": "mission-default" }),
        ));
        assert_eq!(
            log_list[1].results_field().expect("result")["result"]["log_entries"][0]["entry_uid"],
            "log-1"
        );
        assert_eq!(core.log_entries().len(), 1);
        assert_eq!(core.mission_changes().len(), 2);
    }

    #[test]
    fn registry_log_upsert_decodes_mecp_event_content() {
        let mut core = RchCore::new();

        let log_upsert = core.handle_mission_sync_command(&command(
            "mission.registry.log_entry.upsert",
            json!({
                "entry_uid": "mecp-log",
                "content": "MECP/1/R03 T99 4pax 45.5017,-73.5673 #A1 15 @en @0930 ~EAGLE-1 north gate",
            }),
        ));
        let result = &log_upsert[1].results_field().expect("result")["result"];

        assert_eq!(result["entry_uid"], "mecp-log");
        assert_eq!(result["mecp"]["valid"], true);
        assert_eq!(result["mecp"]["severity"], 1);
        assert_eq!(result["mecp"]["category"], "R");
        assert_eq!(result["mecp"]["category_label"], "Response");
        assert_eq!(result["mecp"]["code_details"][0]["label"], "ETA [minutes]");
        assert_eq!(result["mecp"]["extras"]["eta_minutes"], 15);
        assert_eq!(result["mecp"]["extras"]["pax"], 4);
        assert_eq!(
            result["keywords"],
            json!([
                "r3akt:event-type:R",
                "r3akt:event-code:R03",
                "r3akt:event-code:T99"
            ])
        );

        let log_list = core.handle_mission_sync_command(&command(
            "mission.registry.log_entry.list",
            json!({ "mission_uid": "mission-default" }),
        ));
        assert_eq!(
            log_list[1].results_field().expect("result")["result"]["log_entries"][0]["mecp"]["codes"]
                [1],
            "T99"
        );
        assert_eq!(
            core.mission_changes()[0].delta["logs"][0]["mecp"]["extras"]["references"][0],
            "#A1"
        );
    }

    #[test]
    fn registry_log_commands_use_python_rch_capabilities() {
        let mut core = RchCore::new();
        core.set_authorization_required(true);

        let write_rejected = core.handle_mission_sync_command(&command(
            "mission.registry.log_entry.upsert",
            json!({ "content": "blocked" }),
        ));
        core.grant_identity_capability("abcdef", "mission.registry.log.write");
        let write_accepted = core.handle_mission_sync_command(&command(
            "mission.registry.log_entry.upsert",
            json!({ "entry_uid": "auth-log", "content": "allowed" }),
        ));
        let read_rejected = core
            .handle_mission_sync_command(&command("mission.registry.log_entry.list", json!({})));
        core.grant_identity_capability("abcdef", "mission.registry.log.read");
        let read_accepted = core
            .handle_mission_sync_command(&command("mission.registry.log_entry.list", json!({})));

        assert_eq!(
            write_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.registry.log.write"
        );
        assert_eq!(
            write_accepted[1].results_field().expect("result")["status"],
            "result"
        );
        assert_eq!(
            read_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.registry.log.read"
        );
        assert_eq!(
            read_accepted[1].results_field().expect("result")["status"],
            "result"
        );
    }

    #[test]
    fn registry_eam_commands_match_python_surface() {
        let mut core = RchCore::new();
        core.grant_identity_capability("abcdef", "mission.registry.status.write");
        core.grant_identity_capability("abcdef", "mission.registry.status.read");
        let team_uid = "a83eb640e4c4884be14831e3d7ef5ae0";

        let upsert = core.handle_mission_sync_command(&command(
            "mission.registry.eam.upsert",
            json!({
                "callsign": "ORANGE-1",
                "team_member_uid": "member-1",
                "team_uid": team_uid,
                "group_name": "ORANGE",
                "reported_by": "peer-a",
                "security_status": "Green",
                "capability_status": "Yellow",
                "preparedness_status": "Green",
                "medical_status": "Unknown",
                "mobility_status": "Green",
                "comms_status": "Red",
                "notes": "Alternate comms required",
                "confidence": 0.8,
                    "ttl_seconds": 3600,
                "source": {"rns_identity": "peer-a", "display_name": "Peer A"}
            }),
        ));
        assert_eq!(
            upsert[1].event_field().expect("event")["event_type"],
            "mission.registry.eam.upserted"
        );
        let snapshot = &upsert[1].results_field().expect("result")["result"]["eam"];
        assert_eq!(snapshot["callsign"], "ORANGE-1");
        assert_eq!(snapshot["group_name"], "ORANGE");
        assert_eq!(snapshot["overall_status"], "Red");
        assert_eq!(snapshot["source"]["rns_identity"], "peer-a");

        let listed = core.handle_mission_sync_command(&command(
            "mission.registry.eam.list",
            json!({ "team_uid": team_uid }),
        ));
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["eams"][0]["callsign"],
            "ORANGE-1"
        );

        let fetched = core.handle_mission_sync_command(&command(
            "mission.registry.eam.get",
            json!({ "callsign": "ORANGE-1" }),
        ));
        assert_eq!(
            fetched[1].event_field().expect("event")["event_type"],
            "mission.registry.eam.retrieved"
        );

        let latest = core.handle_mission_sync_command(&command(
            "mission.registry.eam.latest",
            json!({ "team_member_uid": "member-1" }),
        ));
        assert_eq!(
            latest[1].results_field().expect("result")["result"]["eam"]["team_member_uid"],
            "member-1"
        );

        let summary = core.handle_mission_sync_command(&command(
            "mission.registry.eam.team.summary",
            json!({ "team_uid": team_uid }),
        ));
        assert_eq!(
            summary[1].event_field().expect("event")["event_type"],
            "mission.registry.eam.team_summary.retrieved"
        );
        assert_eq!(
            summary[1].results_field().expect("result")["result"]["summary"]["red_total"],
            1
        );

        let deleted = core.handle_mission_sync_command(&command(
            "mission.registry.eam.delete",
            json!({ "callsign": "ORANGE-1" }),
        ));
        assert_eq!(
            deleted[1].event_field().expect("event")["event_type"],
            "mission.registry.eam.deleted"
        );
        assert!(core.eam_snapshots()[0].deleted_ts_ms.is_some());
    }

    #[test]
    fn registry_eam_rejections_match_python_contract() {
        let mut core = RchCore::new();
        core.set_authorization_required(true);
        let unauthorized = core.handle_mission_sync_command(&command(
            "mission.registry.eam.upsert",
            json!({ "callsign": "ORANGE-1" }),
        ));
        assert_eq!(
            unauthorized[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.registry.status.write"
        );

        core.grant_identity_capability("abcdef", "mission.registry.status.write");
        let rejected = core.handle_mission_sync_command(&command(
            "mission.registry.eam.upsert",
            json!({
                "callsign": "ORANGE-1",
                "team_member_uid": "member-1",
                "team_uid": "team-orange",
                "securityCapability": "Yellow"
            }),
        ));
        assert_eq!(
            rejected[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
    }

    #[test]
    fn registry_eam_summary_handles_expired_deleted_and_canonical_teams() {
        let mut core = RchCore::new();
        let canonical_orange = "a83eb640e4c4884be14831e3d7ef5ae0";
        let stale = millis_to_rfc3339(utc_now_ms() - 300_000);

        core.handle_command(&command(
            "mission.registry.eam.upsert",
            json!({
                "eam_uid": "eam-expired",
                "callsign": "ORANGE-OLD",
                "team_member_uid": "member-old",
                "team_uid": canonical_orange,
                "reported_at": stale,
                "ttl_seconds": 60,
                "security_status": "Red",
                "source": {"rns_identity": "peer-old"}
            }),
        ));
        core.handle_command(&command(
            "mission.registry.eam.upsert",
            json!({
                "eam_uid": "eam-green",
                "callsign": "ORANGE-2",
                "team_member_uid": "member-green",
                "team_uid": canonical_orange,
                "security_status": "Green",
                "capability_status": "Green",
                "preparedness_status": "Green",
                "medical_status": "Green",
                "mobility_status": "Green",
                "comms_status": "Green",
                "source": {"rns_identity": "peer-green"}
            }),
        ));

        let listed = core.handle_mission_sync_command(&command(
            "mission.registry.eam.list",
            json!({ "team_uid": canonical_orange, "overall_status": "Green" }),
        ));
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["eams"][0]["group_name"],
            "ORANGE"
        );
        let summary = core.handle_mission_sync_command(&command(
            "mission.registry.eam.team.summary",
            json!({ "team_uid": canonical_orange }),
        ));
        assert_eq!(
            summary[1].results_field().expect("result")["result"]["summary"]["total"],
            2
        );
        assert_eq!(
            summary[1].results_field().expect("result")["result"]["summary"]["active_total"],
            1
        );
        assert_eq!(
            summary[1].results_field().expect("result")["result"]["summary"]["deleted_total"],
            1
        );
        assert_eq!(
            summary[1].results_field().expect("result")["result"]["summary"]["overall_status"],
            "Green"
        );
    }

    #[test]
    fn registry_eam_recreate_rejects_conflicting_deleted_subject_and_callsign() {
        let mut core = RchCore::new();
        let canonical_orange = "a83eb640e4c4884be14831e3d7ef5ae0";

        core.handle_command(&command(
            "mission.registry.eam.upsert",
            json!({
                "eam_uid": "eam-subject",
                "callsign": "ORANGE-1",
                "team_member_uid": "member-1",
                "team_uid": canonical_orange,
                "security_status": "Green",
                "source": {"rns_identity": "peer-1"}
            }),
        ));
        core.handle_command(&command(
            "mission.registry.eam.delete",
            json!({ "callsign": "ORANGE-1" }),
        ));

        core.handle_command(&command(
            "mission.registry.eam.upsert",
            json!({
                "eam_uid": "eam-callsign",
                "callsign": "ORANGE-2",
                "team_member_uid": "member-2",
                "team_uid": canonical_orange,
                "security_status": "Yellow",
                "source": {"rns_identity": "peer-2"}
            }),
        ));
        core.handle_command(&command(
            "mission.registry.eam.delete",
            json!({ "callsign": "ORANGE-2" }),
        ));

        let rejected = core.handle_mission_sync_command(&command(
            "mission.registry.eam.upsert",
            json!({
                "callsign": "ORANGE-2",
                "team_member_uid": "member-1",
                "team_uid": canonical_orange,
                "security_status": "Red",
                "source": {"rns_identity": "peer-1"}
            }),
        ));

        assert_eq!(
            rejected[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
        assert!(
            rejected[1].results_field().expect("rejected")["reason"]
                .as_str()
                .expect("reason")
                .contains(
                    "eam_uid cannot be recreated because deleted subject and callsign snapshots refer to different records"
                )
        );
        assert_eq!(core.eam_snapshots().len(), 2);
    }

    #[test]
    fn registry_team_commands_match_python_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-1", "mission_name": "Mission 1" }),
        ));
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-2", "mission_name": "Mission 2" }),
        ));
        let canonical_orange = "a83eb640e4c4884be14831e3d7ef5ae0";

        let canonical_team = core.handle_mission_sync_command(&command(
            "mission.registry.team.upsert",
            json!({ "team_name": "ORANGE" }),
        ));
        assert_eq!(
            canonical_team[1].results_field().expect("result")["result"]["uid"],
            canonical_orange
        );
        assert_eq!(
            canonical_team[1].results_field().expect("result")["result"]["color"],
            "ORANGE"
        );
        assert_eq!(
            canonical_team[1].results_field().expect("result")["result"]["team_name"],
            "ORANGE"
        );
        let explicit_noncanonical_team = core.handle_mission_sync_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": "team-orange", "color": "ORANGE", "team_name": "Ops" }),
        ));
        assert_eq!(
            explicit_noncanonical_team[1]
                .results_field()
                .expect("result")["result"]["uid"],
            "team-orange"
        );
        assert_eq!(
            explicit_noncanonical_team[1]
                .results_field()
                .expect("result")["result"]["team_name"],
            "Ops"
        );
        let ui_colored_team = core.handle_mission_sync_command(&command(
            "mission.registry.team.upsert",
            json!({ "color": "GREEN", "team_name": "UI Smoke Team" }),
        ));
        assert_ne!(
            ui_colored_team[1].results_field().expect("result")["result"]["uid"],
            "612a32262163b73a80eca944c2158546"
        );
        assert_eq!(
            ui_colored_team[1].results_field().expect("result")["result"]["color"],
            "GREEN"
        );
        assert_eq!(
            ui_colored_team[1].results_field().expect("result")["result"]["team_name"],
            "UI Smoke Team"
        );

        let team_upsert = core.handle_mission_sync_command(&command(
            "mission.registry.team.upsert",
            json!({
                "uid": "team-1",
                "team_name": "Bravo",
                "mission_uid": "mission-1",
            }),
        ));
        assert_eq!(
            team_upsert[1].results_field().expect("result")["result"]["uid"],
            "team-1"
        );
        assert_eq!(
            team_upsert[1].event_field().expect("event")["event_type"],
            "mission.registry.team.upserted"
        );
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({ "uid": "member-1", "team_uid": "team-1", "rns_identity": "peer-a" }),
        ));
        assert_eq!(core.team_members()[0].team_uid.as_deref(), Some("team-1"));

        let team_get = core.handle_mission_sync_command(&command(
            "mission.registry.team.get",
            json!({ "team_uid": "team-1" }),
        ));
        assert_eq!(
            team_get[1].results_field().expect("result")["result"]["team_name"],
            "Bravo"
        );

        let team_list = core.handle_mission_sync_command(&command(
            "mission.registry.team.list",
            json!({ "mission_uid": "mission-1" }),
        ));
        assert_eq!(
            team_list[1].results_field().expect("result")["result"]["teams"][0]["uid"],
            "team-1"
        );

        let linked = core.handle_mission_sync_command(&command(
            "mission.registry.team.mission.link",
            json!({ "team_uid": "team-1", "mission_uid": "mission-2" }),
        ));
        assert!(
            linked[1].results_field().expect("result")["result"]["mission_uids"]
                .as_array()
                .expect("mission_uids")
                .iter()
                .any(|value| value == "mission-2")
        );

        let unlinked = core.handle_mission_sync_command(&command(
            "mission.registry.team.mission.unlink",
            json!({ "team_uid": "team-1", "mission_uid": "mission-1" }),
        ));
        assert_eq!(
            unlinked[1].results_field().expect("result")["result"]["mission_uid"],
            "mission-2"
        );

        let deleted = core.handle_mission_sync_command(&command(
            "mission.registry.team.delete",
            json!({ "team_uid": "team-1" }),
        ));
        assert_eq!(
            deleted[1].event_field().expect("event")["event_type"],
            "mission.registry.team.deleted"
        );
        assert!(core.teams().iter().all(|team| team.uid != "team-1"));
        assert!(core.mission_team_links.is_empty());
        assert_eq!(core.team_members()[0].team_uid, None);
    }

    #[test]
    fn registry_team_commands_use_python_rch_capabilities() {
        let mut core = RchCore::new();
        core.set_authorization_required(true);

        let write_rejected = core.handle_mission_sync_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": "team-auth" }),
        ));
        core.grant_identity_capability("abcdef", "mission.registry.team.write");
        let write_accepted = core.handle_mission_sync_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": "team-auth" }),
        ));
        let read_rejected = core.handle_mission_sync_command(&command(
            "mission.registry.team.get",
            json!({ "team_uid": "team-auth" }),
        ));
        core.grant_identity_capability("abcdef", "mission.registry.team.read");
        let read_accepted = core.handle_mission_sync_command(&command(
            "mission.registry.team.get",
            json!({ "team_uid": "team-auth" }),
        ));

        assert_eq!(
            write_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.registry.team.write"
        );
        assert_eq!(
            write_accepted[1].results_field().expect("result")["status"],
            "result"
        );
        assert_eq!(
            read_rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "mission.registry.team.read"
        );
        assert_eq!(
            read_accepted[1].results_field().expect("result")["status"],
            "result"
        );
    }

    #[test]
    fn registry_team_member_commands_match_python_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": "team-1", "team_name": "Alpha" }),
        ));

        let upserted = core.handle_mission_sync_command(&command(
            "mission.registry.team_member.upsert",
            json!({
                "uid": "member-1",
                "team_uid": "team-1",
                "rns_identity": "peer-b",
                "display_name": "Peer B",
            }),
        ));
        assert_eq!(
            upserted[1].results_field().expect("result")["result"]["uid"],
            "member-1"
        );

        let listed = core.handle_mission_sync_command(&command(
            "mission.registry.team_member.list",
            json!({ "team_uid": "team-1" }),
        ));
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["team_members"][0]["uid"],
            "member-1"
        );

        let linked = core.handle_mission_sync_command(&command(
            "mission.registry.team_member.client.link",
            json!({ "team_member_uid": "member-1", "client_identity": "PEER-A" }),
        ));
        assert_eq!(
            linked[1].results_field().expect("result")["result"]["client_identities"][0],
            "peer-a"
        );
        core.handle_mission_sync_command(&command(
            "mission.registry.asset.upsert",
            json!({ "asset_uid": "asset-1", "team_member_uid": "member-1", "name": "Radio" }),
        ));
        core.handle_mission_sync_command(&command(
            "mission.registry.skill.upsert",
            json!({ "skill_uid": "skill-1", "name": "Navigation" }),
        ));
        core.handle_mission_sync_command(&command(
            "mission.registry.team_member_skill.upsert",
            json!({
                "uid": "member-skill-1",
                "team_member_rns_identity": "peer-b",
                "skill_uid": "skill-1",
                "level": 3
            }),
        ));
        assert_eq!(
            core.assets()[0].team_member_uid.as_deref(),
            Some("member-1")
        );
        assert_eq!(core.team_member_skills().len(), 1);

        let retrieved = core.handle_mission_sync_command(&command(
            "mission.registry.team_member.get",
            json!({ "team_member_uid": "member-1" }),
        ));
        assert_eq!(
            retrieved[1].event_field().expect("event")["event_type"],
            "mission.registry.team_member.retrieved"
        );

        let deleted = core.handle_mission_sync_command(&command(
            "mission.registry.team_member.delete",
            json!({ "team_member_uid": "member-1" }),
        ));
        assert_eq!(
            deleted[1].event_field().expect("event")["event_type"],
            "mission.registry.team_member.deleted"
        );
        assert!(core.team_members().is_empty());
        assert!(core.team_member_client_links.is_empty());
        assert_eq!(core.assets()[0].team_member_uid, None);
        assert!(core.team_member_skills().is_empty());
    }

    #[test]
    fn registry_asset_commands_match_python_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-1", "mission_name": "Mission One" }),
        ));
        core.handle_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": "team-1", "team_name": "Alpha", "mission_uid": "mission-1" }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({ "uid": "member-1", "team_uid": "team-1", "rns_identity": "peer-a" }),
        ));
        seed_checklist_with_task(&mut core);

        let upserted = core.handle_mission_sync_command(&command(
            "mission.registry.asset.upsert",
            json!({
                "asset_uid": "asset-1",
                "team_member_uid": "member-1",
                "name": "Battery Pack",
                "asset_type": "POWER",
            }),
        ));
        assert_eq!(
            upserted[1].results_field().expect("result")["result"]["asset_uid"],
            "asset-1"
        );
        let upsert_change = core
            .mission_changes()
            .into_iter()
            .find(|change| change.name.as_deref() == Some("mission.asset.upserted"))
            .expect("asset upsert change");
        assert_eq!(upsert_change.mission_uid, "mission-1");
        assert_eq!(upsert_change.change_type, "ADD_CONTENT");
        assert_eq!(upsert_change.delta["assets"][0]["op"], "upsert");
        assert_eq!(upsert_change.delta["assets"][0]["asset_uid"], "asset-1");

        let listed = core.handle_mission_sync_command(&command(
            "mission.registry.asset.list",
            json!({ "team_member_uid": "member-1" }),
        ));
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["assets"][0]["name"],
            "Battery Pack"
        );

        let retrieved = core.handle_mission_sync_command(&command(
            "mission.registry.asset.get",
            json!({ "asset_uid": "asset-1" }),
        ));
        assert_eq!(
            retrieved[1].event_field().expect("event")["event_type"],
            "mission.registry.asset.retrieved"
        );

        let assigned = core.handle_mission_sync_command(&command(
            "mission.registry.assignment.upsert",
            json!({
                "assignment_uid": "assignment-1",
                "mission_uid": "mission-1",
                "task_uid": "task-1",
                "team_member_rns_identity": "peer-a",
                "assets": ["asset-1"]
            }),
        ));
        assert_eq!(
            assigned[1].results_field().expect("result")["result"]["assets"][0],
            "asset-1"
        );

        let deleted = core.handle_mission_sync_command(&command(
            "mission.registry.asset.delete",
            json!({ "asset_uid": "asset-1" }),
        ));
        assert_eq!(
            deleted[1].event_field().expect("event")["event_type"],
            "mission.registry.asset.deleted"
        );
        assert!(core.assets().is_empty());
        assert!(core.assignments()[0].assets.is_empty());
        let delete_change = core
            .mission_changes()
            .into_iter()
            .find(|change| change.name.as_deref() == Some("mission.asset.deleted"))
            .expect("asset delete change");
        assert_eq!(delete_change.mission_uid, "mission-1");
        assert_eq!(delete_change.change_type, "REMOVE_CONTENT");
        assert_eq!(delete_change.delta["assets"][0]["op"], "delete");
        assert_eq!(delete_change.delta["assets"][0]["asset_uid"], "asset-1");
    }

    #[test]
    fn registry_skill_commands_match_python_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({ "uid": "member-1", "rns_identity": "peer-a" }),
        ));

        let skill_upsert = core.handle_mission_sync_command(&command(
            "mission.registry.skill.upsert",
            json!({ "skill_uid": "skill-1", "name": "Navigation" }),
        ));
        assert_eq!(
            skill_upsert[1].results_field().expect("result")["result"]["skill_uid"],
            "skill-1"
        );

        let skill_list =
            core.handle_mission_sync_command(&command("mission.registry.skill.list", json!({})));
        assert_eq!(
            skill_list[1].results_field().expect("result")["result"]["skills"][0]["name"],
            "Navigation"
        );

        let member_skill = core.handle_mission_sync_command(&command(
            "mission.registry.team_member_skill.upsert",
            json!({
                "uid": "member-skill-1",
                "team_member_rns_identity": "peer-a",
                "skill_uid": "skill-1",
                "level": 3,
            }),
        ));
        assert_eq!(
            member_skill[1].results_field().expect("result")["result"]["uid"],
            "member-skill-1"
        );
        let now = utc_now_ms();
        core.checklist_tasks.insert(
            "task-1".to_string(),
            ChecklistTaskRecord {
                task_uid: "task-1".to_string(),
                checklist_uid: "checklist-1".to_string(),
                number: 1,
                user_status: "PENDING".to_string(),
                task_status: "PENDING".to_string(),
                is_late: false,
                custom_status: None,
                due_relative_minutes: None,
                due_ts_ms: None,
                notes: None,
                row_background_color: None,
                line_break_enabled: false,
                completed_ts_ms: None,
                completed_by_team_member_rns_identity: None,
                legacy_value: None,
                created_ts_ms: now,
                updated_ts_ms: now,
            },
        );

        let requirements = core.handle_mission_sync_command(&command(
            "mission.registry.task_skill_requirement.upsert",
            json!({
                "uid": "requirement-1",
                "task_uid": "task-1",
                "skill_uid": "skill-1",
                "minimum_level": 2,
            }),
        ));
        assert_eq!(
            requirements[1].results_field().expect("result")["result"]["minimum_level"],
            2
        );
    }

    #[test]
    fn registry_assignment_commands_match_python_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-1", "mission_name": "Mission One" }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({ "uid": "member-1", "rns_identity": "peer-a" }),
        ));
        core.handle_command(&command(
            "mission.registry.asset.upsert",
            json!({ "asset_uid": "asset-1", "name": "Radio" }),
        ));
        core.handle_command(&command(
            "mission.registry.asset.upsert",
            json!({ "asset_uid": "asset-2", "name": "Battery" }),
        ));
        let now = utc_now_ms();
        core.checklist_tasks.insert(
            "task-1".to_string(),
            ChecklistTaskRecord {
                task_uid: "task-1".to_string(),
                checklist_uid: "checklist-1".to_string(),
                number: 1,
                user_status: "PENDING".to_string(),
                task_status: "PENDING".to_string(),
                is_late: false,
                custom_status: None,
                due_relative_minutes: None,
                due_ts_ms: None,
                notes: None,
                row_background_color: None,
                line_break_enabled: false,
                completed_ts_ms: None,
                completed_by_team_member_rns_identity: None,
                legacy_value: None,
                created_ts_ms: now,
                updated_ts_ms: now,
            },
        );

        let upserted = core.handle_mission_sync_command(&command(
            "mission.registry.assignment.upsert",
            json!({
                "assignment_uid": "assignment-1",
                "mission_uid": "mission-1",
                "task_uid": "task-1",
                "team_member_rns_identity": "peer-a",
                "assets": ["asset-1"],
            }),
        ));
        assert_eq!(
            upserted[1].results_field().expect("result")["result"]["assignment_uid"],
            "assignment-1"
        );
        let assignment_change = core
            .mission_changes()
            .into_iter()
            .find(|change| change.name.as_deref() == Some("mission.assignment.upserted"))
            .expect("assignment upsert change");
        assert_eq!(assignment_change.mission_uid, "mission-1");
        assert_eq!(assignment_change.change_type, "ADD_CONTENT");
        assert_eq!(
            assignment_change.team_member_rns_identity.as_deref(),
            Some("peer-a")
        );
        assert_eq!(
            assignment_change.delta["tasks"][0]["op"],
            "assignment_upsert"
        );
        assert_eq!(
            assignment_change.delta["tasks"][0]["assignment_uid"],
            "assignment-1"
        );

        let linked = core.handle_mission_sync_command(&command(
            "mission.registry.assignment.asset.link",
            json!({ "assignment_uid": "assignment-1", "asset_uid": "asset-2" }),
        ));
        assert_eq!(
            linked[1].results_field().expect("result")["result"]["assets"],
            json!(["asset-1", "asset-2"])
        );
        let link_change = core
            .mission_changes()
            .into_iter()
            .find(|change| change.name.as_deref() == Some("mission.assignment.asset.linked"))
            .expect("assignment asset link change");
        assert_eq!(link_change.change_type, "ADD_CONTENT");
        assert_eq!(
            link_change.delta["tasks"][0]["op"],
            "assignment_asset_linked"
        );
        assert_eq!(link_change.delta["tasks"][0]["asset_uid"], "asset-2");

        let set = core.handle_mission_sync_command(&command(
            "mission.registry.assignment.asset.set",
            json!({ "assignment_uid": "assignment-1", "assets": ["asset-2"] }),
        ));
        assert_eq!(
            set[1].results_field().expect("result")["result"]["assets"],
            json!(["asset-2"])
        );
        let set_change = core
            .mission_changes()
            .into_iter()
            .find(|change| change.name.as_deref() == Some("mission.assignment.assets.updated"))
            .expect("assignment asset set change");
        assert_eq!(set_change.change_type, "ADD_CONTENT");
        assert_eq!(set_change.delta["tasks"][0]["op"], "assignment_assets_set");
        assert_eq!(set_change.delta["tasks"][0]["assets"], json!(["asset-2"]));

        let listed = core.handle_mission_sync_command(&command(
            "mission.registry.assignment.list",
            json!({ "mission_uid": "mission-1" }),
        ));
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["assignments"][0]["task_uid"],
            "task-1"
        );

        let unlinked = core.handle_mission_sync_command(&command(
            "mission.registry.assignment.asset.unlink",
            json!({ "assignment_uid": "assignment-1", "asset_uid": "asset-2" }),
        ));
        assert_eq!(
            unlinked[1].results_field().expect("result")["result"]["assets"],
            json!([])
        );
        let unlink_change = core
            .mission_changes()
            .into_iter()
            .find(|change| change.name.as_deref() == Some("mission.assignment.asset.unlinked"))
            .expect("assignment asset unlink change");
        assert_eq!(unlink_change.change_type, "REMOVE_CONTENT");
        assert_eq!(
            unlink_change.delta["tasks"][0]["op"],
            "assignment_asset_unlinked"
        );
        assert_eq!(unlink_change.delta["tasks"][0]["asset_uid"], "asset-2");
        assert_eq!(unlink_change.delta["tasks"][0]["assets"], json!([]));
    }

    #[test]
    fn registry_rights_commands_match_python_route_surface() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({
                "uid": "mission-1",
                "mission_name": "Mission One",
                "default_role": "MISSION_SUBSCRIBER",
            }),
        ));
        core.handle_command(&command(
            "mission.registry.team.upsert",
            json!({
                "uid": "team-1",
                "mission_uid": "mission-1",
                "team_name": "Ops",
            }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({
                "uid": "member-1",
                "team_uid": "team-1",
                "rns_identity": "peer-a",
                "display_name": "Peer A",
            }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member.client.link",
            json!({ "team_member_uid": "member-1", "client_identity": "peer-a" }),
        ));

        let subjects = core.handle_mission_sync_command(&command(
            "mission.registry.rights.subjects.list",
            json!({ "mission_uid": "mission-1" }),
        ));
        let subject = &subjects[1].results_field().expect("result")["result"]["subjects"][0];
        assert_eq!(subject["subject_id"], "member-1");
        assert_eq!(subject["client_identities"], json!(["peer-a"]));

        let assigned = core.handle_mission_sync_command(&command(
            "mission.registry.rights.mission_access.assign",
            json!({
                "mission_uid": "mission-1",
                "subject_type": "team_member",
                "subject_id": "member-1",
            }),
        ));
        assert_eq!(
            assigned[1].results_field().expect("result")["result"]["role"],
            "MISSION_SUBSCRIBER"
        );

        let listed = core.handle_mission_sync_command(&command(
            "mission.registry.rights.mission_access.list",
            json!({ "mission_uid": "mission-1" }),
        ));
        assert_eq!(
            listed[1].results_field().expect("result")["result"]["mission_access_assignments"][0]["subject_id"],
            "member-1"
        );

        let revoked = core.handle_mission_sync_command(&command(
            "mission.registry.rights.mission_access.revoke",
            json!({
                "mission_uid": "mission-1",
                "subject_type": "team_member",
                "subject_id": "member-1",
            }),
        ));
        assert_eq!(
            revoked[1].results_field().expect("result")["result"]["deleted"],
            true
        );
    }

    #[test]
    fn explicit_operation_revoke_overrides_mission_role_after_sqlite_restore() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-1", "mission_name": "Mission One" }),
        ));
        core.handle_command(&command(
            "mission.registry.team.upsert",
            json!({
                "uid": "team-1",
                "mission_uid": "mission-1",
                "team_name": "Ops",
            }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({
                "uid": "member-1",
                "team_uid": "team-1",
                "rns_identity": "peer-member",
                "display_name": "Peer Member",
            }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member.client.link",
            json!({ "team_member_uid": "member-1", "client_identity": "peer-client" }),
        ));
        core.assign_mission_access_role(
            "mission-1",
            "team_member",
            "member-1",
            "MISSION_SUBSCRIBER",
        )
        .expect("mission access assignment");
        assert!(core.authorize_identity_operation(
            "peer-client",
            "mission.message.send",
            Some("mission-1")
        ));

        core.revoke_operation_right(
            "team_member",
            "member-1",
            "mission.message.send",
            "mission",
            "mission-1",
        )
        .expect("operation revoke");
        assert!(!core.authorize_identity_operation(
            "peer-client",
            "mission.message.send",
            Some("mission-1")
        ));

        let mut store = RchSqliteStore::in_memory().expect("store");
        core.save_to_sqlite(&mut store).expect("save");
        let restored = RchCore::load_from_sqlite(&store)
            .expect("load")
            .expect("snapshot");
        assert!(!restored.authorize_identity_operation(
            "peer-client",
            "mission.message.send",
            Some("mission-1")
        ));
    }

    #[test]
    fn persona_role_bundles_define_release_roles_and_grant_expected_operations() {
        let definitions = rch_role_bundle_definitions();
        for role in [
            ROLE_FIELD_OPERATOR,
            ROLE_TEAM_LEAD,
            ROLE_INCIDENT_COMMANDER,
            ROLE_LOGISTICS_RESOURCE_MANAGER,
            ROLE_COMMUNICATIONS_OPERATOR,
            ROLE_SYSTEM_ADMIN,
        ] {
            assert!(
                definitions.iter().any(|bundle| bundle.role == role),
                "{role} bundle missing"
            );
        }

        let mission_bundles = rch_mission_role_bundle_definitions();
        assert!(mission_bundles[ROLE_FIELD_OPERATOR].contains(&"mission.message.send"));
        assert!(mission_bundles[ROLE_FIELD_OPERATOR].contains(&"checklist.write"));
        assert!(mission_bundles[ROLE_TEAM_LEAD].contains(&"mission.registry.assignment.write"));
        assert!(
            mission_bundles[ROLE_INCIDENT_COMMANDER].contains(&"mission.registry.mission.write")
        );
        assert!(
            mission_bundles[ROLE_LOGISTICS_RESOURCE_MANAGER]
                .contains(&"mission.registry.asset.write")
        );
        assert!(mission_bundles[ROLE_COMMUNICATIONS_OPERATOR].contains(&"runtime.routing.read"));
        assert!(!mission_bundles.contains_key(ROLE_SYSTEM_ADMIN));
        assert!(rch_operation_definitions().contains(&"admin.config.write"));

        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-roles", "mission_name": "Mission Roles" }),
        ));
        core.handle_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": "team-roles", "mission_uid": "mission-roles", "team_name": "Ops" }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({
                "uid": "member-roles",
                "team_uid": "team-roles",
                "rns_identity": "peer-roles",
                "display_name": "Peer Roles",
                "role": ROLE_FIELD_OPERATOR
            }),
        ));
        core.assign_mission_access_role(
            "mission-roles",
            "team_member",
            "member-roles",
            ROLE_TEAM_LEAD,
        )
        .expect("team lead mission bundle");
        assert!(core.authorize_identity_operation(
            "peer-roles",
            "mission.registry.assignment.write",
            Some("mission-roles")
        ));
        assert!(!core.authorize_identity_operation(
            "peer-roles",
            "admin.config.write",
            Some("mission-roles")
        ));
    }

    #[test]
    fn checklist_sync_success_is_silent_and_updates_state() {
        let mut core = RchCore::new();
        seed_checklist_with_task(&mut core);

        let style = command(
            "checklist.task.row.style.set",
            json!({
                "checklist_uid": "checklist-1",
                "task_uid": "task-1",
                "row_background_color": "#abc123",
                "line_break_enabled": true
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&style), []);
        assert!(core.checklist_tasks()[0].line_break_enabled);

        let status = command(
            "checklist.task.status.set",
            json!({
                "checklist_uid": "checklist-1",
                "task_uid": "task-1",
                "user_status": "COMPLETE",
                "changed_by_team_member_rns_identity": "peer-a"
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&status), []);
        let checklist = core.checklists().pop().expect("checklist");
        assert_eq!(checklist.complete_count, 1);
        assert_eq!(checklist.checklist_status, "COMPLETE");

        let update = command(
            "checklist.update",
            json!({
                "checklist_uid": "checklist-1",
                "patch": { "name": "Updated Checklist" }
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&update), []);
        assert_eq!(core.checklists()[0].name, "Updated Checklist");

        let upload = command(
            "checklist.upload",
            json!({ "checklist_uid": "checklist-1" }),
        );
        assert_eq!(core.handle_checklist_sync_command(&upload), []);
        assert_eq!(core.checklists()[0].sync_state, "SYNCED");

        let feed_publish = command(
            "checklist.feed.publish",
            json!({
                "checklist_uid": "checklist-1",
                "mission_feed_uid": "feed-1"
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&feed_publish), []);
        assert_eq!(core.checklist_feed_publications().len(), 1);
        assert_eq!(
            core.checklist_value(&core.checklists()[0])["feed_publications"][0]["mission_feed_uid"],
            "feed-1"
        );

        let join = command("checklist.join", json!({ "checklist_uid": "checklist-1" }));
        assert_eq!(core.handle_checklist_sync_command(&join), []);

        let delete_row = command(
            "checklist.task.row.delete",
            json!({ "checklist_uid": "checklist-1", "task_uid": "task-1" }),
        );
        assert_eq!(core.handle_checklist_sync_command(&delete_row), []);
        assert!(core.checklist_tasks().is_empty());

        let delete = command(
            "checklist.delete",
            json!({ "checklist_uid": "checklist-1" }),
        );
        assert_eq!(core.handle_checklist_sync_command(&delete), []);
        assert!(core.checklists().is_empty());
    }

    #[test]
    fn standalone_checklist_task_status_emits_mission_change_for_current_user_fanout() {
        let mut core = RchCore::new();
        let created = core.handle_command(&command(
            "checklist.create.offline",
            json!({
                "checklist_uid": "standalone-checklist",
                "name": "Standalone Checklist"
            }),
        ));
        assert_eq!(created.result.status, CommandResultStatus::Accepted);
        let added = core.handle_command(&command(
            "checklist.task.row.add",
            json!({
                "checklist_uid": "standalone-checklist",
                "task_uid": "standalone-task",
                "number": 1,
                "legacy_value": "Confirm relay"
            }),
        ));
        assert_eq!(added.result.status, CommandResultStatus::Accepted);

        let completed = core.handle_command(&command(
            "checklist.task.status.set",
            json!({
                "checklist_uid": "standalone-checklist",
                "task_uid": "standalone-task",
                "user_status": "COMPLETE",
                "changed_by_team_member_rns_identity": "ui.operator"
            }),
        ));
        assert_eq!(completed.result.status, CommandResultStatus::Accepted);

        let status_change = core
            .mission_changes()
            .into_iter()
            .find(|change| change.name.as_deref() == Some("mission.checklist.task.status_set"))
            .expect("standalone status mission change");
        assert_eq!(status_change.mission_uid, "");
        assert_eq!(
            status_change.delta["tasks"][0]["checklist_uid"],
            "standalone-checklist"
        );
        assert_eq!(
            status_change.delta["tasks"][0]["task_uid"],
            "standalone-task"
        );
        assert_eq!(status_change.delta["tasks"][0]["number"], 1);
        assert_eq!(status_change.delta["tasks"][0]["user_status"], "COMPLETE");
    }

    #[test]
    fn checklist_delete_cleans_assignment_links_like_python_cascade() {
        let mut core = RchCore::new();
        seed_checklist_with_task(&mut core);
        let _ = core.handle_mission_sync_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-1", "mission_name": "Mission One" }),
        ));
        let _ = core.handle_mission_sync_command(&command(
            "mission.registry.team_member.upsert",
            json!({ "uid": "member-1", "rns_identity": "peer-a" }),
        ));
        let asset = command(
            "mission.registry.asset.upsert",
            json!({ "asset_uid": "asset-1", "name": "Radio" }),
        );
        let _ = core.handle_mission_sync_command(&asset);
        let skill = command(
            "mission.registry.skill.upsert",
            json!({ "skill_uid": "skill-1", "name": "Radio Operator" }),
        );
        let _ = core.handle_mission_sync_command(&skill);
        let requirement = command(
            "mission.registry.task_skill_requirement.upsert",
            json!({
                "uid": "requirement-1",
                "task_uid": "task-1",
                "skill_uid": "skill-1",
                "minimum_level": 2,
                "is_mandatory": true
            }),
        );
        let _ = core.handle_mission_sync_command(&requirement);
        let assignment = command(
            "mission.registry.assignment.upsert",
            json!({
                "assignment_uid": "assignment-1",
                "mission_uid": "mission-1",
                "task_uid": "task-1",
                "team_member_rns_identity": "peer-a",
                "assets": ["asset-1"]
            }),
        );
        let _ = core.handle_mission_sync_command(&assignment);
        assert_eq!(core.assignments().len(), 1);
        assert_eq!(core.task_skill_requirements().len(), 1);
        assert_eq!(core.snapshot().assignment_asset_links.len(), 1);
        let mission_change_count = core.mission_changes().len();

        let delete = command(
            "checklist.delete",
            json!({ "checklist_uid": "checklist-1" }),
        );
        assert_eq!(core.handle_checklist_sync_command(&delete), []);
        assert!(core.checklists().is_empty());
        assert!(core.checklist_tasks().is_empty());
        assert!(core.checklist_cells().is_empty());
        assert!(core.task_skill_requirements().is_empty());
        assert!(core.assignments().is_empty());
        assert!(core.snapshot().assignment_asset_links.is_empty());
        assert_eq!(
            core.mission_changes().len(),
            mission_change_count,
            "Python checklist.delete records checklist.deleted without an auto mission-change"
        );
        assert!(
            core.audit_events()
                .iter()
                .any(|event| event.event_type == "checklist.deleted"
                    && event.payload["uid"] == "checklist-1")
        );
    }

    #[test]
    fn checklist_row_add_with_existing_task_uid_updates_like_python_without_new_change() {
        let mut core = RchCore::new();
        let _ = core.handle_mission_sync_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-1", "mission_name": "Mission One" }),
        ));
        assert_eq!(core.mission_changes().len(), 0);

        let create = command(
            "checklist.create.online",
            json!({
                "checklist_uid": "checklist-1",
                "mission_uid": "mission-1",
                "origin_type": "BLANK_TEMPLATE",
                "name": "Online Checklist",
                "columns": [
                    {
                        "column_name": "Due",
                        "column_type": "RELATIVE_TIME",
                        "column_editable": false,
                        "is_removable": false,
                        "system_key": "DUE_RELATIVE_DTG"
                    },
                    {
                        "column_name": "Task",
                        "column_type": "SHORT_STRING"
                    }
                ]
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&create), []);

        let add_row = command(
            "checklist.task.row.add",
            json!({
                "checklist_uid": "checklist-1",
                "task_uid": "task-1",
                "number": 1,
                "legacy_value": "Initial",
                "notes": "First note"
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&add_row), []);
        let cell_count = core.checklist_cells().len();
        assert_eq!(core.checklist_tasks().len(), 1);
        assert_eq!(core.mission_changes().len(), 2);

        let update_existing = command(
            "checklist.task.row.add",
            json!({
                "checklist_uid": "checklist-1",
                "task_uid": "task-1",
                "number": 7,
                "legacy_value": "Updated",
                "notes": "Updated note"
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&update_existing), []);

        let tasks = core.checklist_tasks();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].number, 7);
        assert_eq!(tasks[0].legacy_value.as_deref(), Some("Updated"));
        assert_eq!(tasks[0].notes.as_deref(), Some("Updated note"));
        assert_eq!(core.checklist_cells().len(), cell_count);
        assert_eq!(
            core.mission_changes().len(),
            2,
            "Python returns before emitting a row-added mission change for existing task_uid"
        );
    }

    #[test]
    fn checklist_csv_import_matches_python_router_shape() {
        let mut core = RchCore::new();
        let csv_payload = BASE64_STANDARD.encode("Due,Task\n10,Task 1\n20,Task 2\n");
        let import = command(
            "checklist.import.csv",
            json!({
                "csv_filename": "import.csv",
                "csv_base64": csv_payload
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&import), []);
        let imported = core
            .checklists()
            .into_iter()
            .find(|checklist| checklist.origin_type == "CSV_IMPORT")
            .expect("imported checklist");
        assert_eq!(imported.mode, "ONLINE");
        assert_eq!(imported.sync_state, "SYNCED");
        assert_eq!(core.checklist_tasks().len(), 2);
        assert!(
            core.checklist_cells()
                .iter()
                .any(|cell| cell.value.as_deref() == Some("Task 1"))
        );
    }

    #[test]
    fn checklist_csv_import_uses_python_default_due_step_without_due_column() {
        let mut core = RchCore::new();
        let csv_payload =
            BASE64_STANDARD.encode("Task,Description\nTask 1,Inspect\nTask 2,Secure\n");
        let import = command(
            "checklist.import.csv",
            json!({
                "csv_filename": "import.csv",
                "csv_base64": csv_payload
            }),
        );

        assert_eq!(core.handle_checklist_sync_command(&import), []);

        let due_offsets: Vec<_> = core
            .checklist_tasks()
            .iter()
            .map(|task| task.due_relative_minutes)
            .collect();
        assert_eq!(due_offsets, vec![Some(30), Some(60)]);
    }

    #[test]
    fn checklist_csv_import_ignores_invalid_utf8_and_keeps_quoted_cells_like_python() {
        let mut core = RchCore::new();
        let csv_payload =
            BASE64_STANDARD.encode(b"Task,Description\nTa\xffsk 1,\"Inspect, north ridge\"\n");
        let import = command(
            "checklist.import.csv",
            json!({
                "csv_filename": "quoted.csv",
                "csv_base64": csv_payload,
                "source_identity": "peer-a"
            }),
        );

        assert_eq!(core.handle_checklist_sync_command(&import), []);

        let imported = core
            .checklists()
            .into_iter()
            .find(|checklist| checklist.origin_type == "CSV_IMPORT")
            .expect("imported checklist");
        let task_column = core
            .checklist_columns()
            .into_iter()
            .find(|column| {
                column.checklist_uid.as_deref() == Some(imported.uid.as_str())
                    && column.column_name == "Task"
            })
            .expect("task column");
        let description_column = core
            .checklist_columns()
            .into_iter()
            .find(|column| {
                column.checklist_uid.as_deref() == Some(imported.uid.as_str())
                    && column.column_name == "Description"
            })
            .expect("description column");
        let task = core
            .checklist_tasks()
            .into_iter()
            .find(|task| task.checklist_uid == imported.uid)
            .expect("task");
        let cells = core.checklist_cells();
        let task_cell = cells
            .iter()
            .find(|cell| {
                cell.task_uid == task.task_uid && cell.column_uid == task_column.column_uid
            })
            .expect("task cell");
        let description_cell = cells
            .iter()
            .find(|cell| {
                cell.task_uid == task.task_uid && cell.column_uid == description_column.column_uid
            })
            .expect("description cell");
        assert_eq!(task.legacy_value.as_deref(), Some("Task 1"));
        assert_eq!(task_cell.value.as_deref(), Some("Task 1"));
        assert_eq!(
            description_cell.value.as_deref(),
            Some("Inspect, north ridge")
        );
    }

    fn seed_checklist_with_task(core: &mut RchCore) {
        let create = command(
            "checklist.create.offline",
            json!({
                "checklist_uid": "checklist-1",
                "origin_type": "BLANK_TEMPLATE",
                "name": "Offline Checklist"
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&create), []);
        assert_eq!(core.checklists().len(), 1);

        let add_row = command(
            "checklist.task.row.add",
            json!({
                "checklist_uid": "checklist-1",
                "task_uid": "task-1",
                "number": 1,
                "legacy_value": "Inspect area"
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&add_row), []);
        assert_eq!(core.checklist_tasks().len(), 1);
        let task_column_uid = core
            .checklist_columns()
            .into_iter()
            .find(|column| column.system_key.is_none() && column.column_type == "SHORT_STRING")
            .expect("editable task column")
            .column_uid
            .clone();

        let cell_set = command(
            "checklist.task.cell.set",
            json!({
                "checklist_uid": "checklist-1",
                "task_uid": "task-1",
                "column_uid": task_column_uid,
                "value": "Inspect area",
                "updated_by_team_member_rns_identity": "peer-a"
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&cell_set), []);
        assert!(
            core.checklist_cells()
                .iter()
                .any(|cell| cell.value.as_deref() == Some("Inspect area"))
        );
    }

    #[test]
    fn checklist_sync_rejections_match_python_silent_success_boundary() {
        let mut core = RchCore::new();
        let unknown = core.handle_checklist_sync_command(&command("checklist.unknown", json!({})));
        assert_eq!(unknown.len(), 1);
        assert_eq!(
            unknown[0].results_field().expect("rejected")["reason_code"],
            "unknown_command"
        );

        core.set_authorization_required(true);
        let unauthorized = core.handle_checklist_sync_command(&command(
            "checklist.create.offline",
            json!({ "name": "Offline" }),
        ));
        assert_eq!(
            unauthorized[0].results_field().expect("rejected")["reason_code"],
            "unauthorized"
        );
    }

    #[test]
    fn checklist_template_commands_support_online_creation() {
        let mut core = RchCore::new();
        let create_template = command(
            "checklist.template.create",
            json!({
                "template": {
                    "uid": "template-1",
                    "template_name": "Template Alpha",
                    "created_by_team_member_rns_identity": "peer-a",
                    "columns": default_checklist_columns()
                }
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&create_template), []);
        assert_eq!(core.checklist_templates().len(), 1);

        let update_template = command(
            "checklist.template.update",
            json!({
                "template_uid": "template-1",
                "patch": { "template_name": "Template Beta" }
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&update_template), []);
        assert_eq!(core.checklist_templates()[0].template_name, "Template Beta");

        let clone_template = command(
            "checklist.template.clone",
            json!({
                "source_template_uid": "template-1",
                "template_name": "Template Clone"
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&clone_template), []);
        assert_eq!(core.checklist_templates().len(), 2);

        let create_online = command(
            "checklist.create.online",
            json!({
                "checklist_uid": "checklist-online",
                "template_uid": "template-1",
                "name": "Checklist Online"
            }),
        );
        assert_eq!(core.handle_checklist_sync_command(&create_online), []);
        assert_eq!(core.checklists()[0].mode, "ONLINE");
        assert_eq!(core.checklist_columns().len(), 6);

        let create_online_with_rows = command(
            "checklist.create.online",
            json!({
                "checklist_uid": "checklist-online-with-rows",
                "name": "Checklist Online With Rows",
                "columns": default_checklist_columns(),
                "tasks": [
                    {
                        "number": 1,
                        "legacy_value": "Pack radio",
                        "row_background_color": "#112233",
                        "line_break_enabled": true,
                        "cells": [
                            {
                                "column_order": 1,
                                "value": "Pack radio",
                                "updated_by_team_member_rns_identity": "peer-a"
                            }
                        ]
                    }
                ]
            }),
        );
        assert_eq!(
            core.handle_checklist_sync_command(&create_online_with_rows),
            []
        );
        let batched_task = core
            .checklist_tasks()
            .into_iter()
            .find(|task| task.checklist_uid == "checklist-online-with-rows")
            .expect("batched task");
        assert_eq!(batched_task.legacy_value.as_deref(), Some("Pack radio"));
        assert_eq!(
            batched_task.row_background_color.as_deref(),
            Some("#112233")
        );
        assert!(batched_task.line_break_enabled);
        assert!(core.checklist_cells().iter().any(|cell| {
            cell.task_uid == batched_task.task_uid
                && cell.value.as_deref() == Some("Pack radio")
                && cell.updated_by_team_member_rns_identity.as_deref() == Some("peer-a")
        }));

        let delete_template = command(
            "checklist.template.delete",
            json!({ "template_uid": "template-1" }),
        );
        assert_eq!(core.handle_checklist_sync_command(&delete_template), []);
        assert_eq!(core.checklist_templates().len(), 1);
    }

    #[test]
    fn mission_message_send_records_python_rch_mixed_topic_destination_shape() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "CreateTopic",
            json!({ "TopicID": "mission-1", "TopicPath": "mission-1", "TopicName": "Mission 1" }),
        ));

        let sent = core.handle_command(&command(
            "mission.message.send",
            json!({ "content": "hello", "topic_id": "mission-1" }),
        ));
        let mixed = core.handle_command(&command(
            "mission.message.send",
            json!({ "content": "mixed", "topic_id": "mission-1", "destination": "abcd" }),
        ));

        assert_eq!(sent.result.status, CommandResultStatus::Accepted);
        assert_eq!(core.messages()[0].delivery_mode, DeliveryMode::Fanout);
        assert_eq!(mixed.result.status, CommandResultStatus::Accepted);
        assert_eq!(core.messages()[1].delivery_mode, DeliveryMode::Fanout);
        assert_eq!(core.messages()[1].destination.as_deref(), Some("abcd"));
    }

    #[test]
    fn mission_sync_success_emits_accepted_then_result_with_event() {
        let mut core = RchCore::new();
        let command = command(
            "topic.create",
            json!({
                "topic_id": "mission-alpha",
                "topic_path": "mission-alpha",
                "topic_name": "Mission Alpha",
                "visibility": "public",
            }),
        );

        let responses = core.handle_mission_sync_command(&command);

        assert_eq!(responses.len(), 2);
        assert_eq!(
            responses[0].results_field().expect("accepted")["status"],
            "accepted"
        );
        assert!(
            responses[0].results_field().expect("accepted")["accepted_at"]
                .as_str()
                .is_some()
        );
        assert_eq!(
            responses[1].results_field().expect("result")["status"],
            "result"
        );
        assert_eq!(
            responses[1].event_field().expect("event")["event_type"],
            "mission.topic.created"
        );
        assert_eq!(core.topics().len(), 1);
    }

    #[test]
    fn mission_sync_execution_error_keeps_accepted_then_rejected_shape() {
        let mut core = RchCore::new();
        let command = command(
            "mission.message.send",
            json!({ "content": " ", "topic_id": "missing-topic" }),
        );

        let responses = core.handle_mission_sync_command(&command);

        assert_eq!(responses.len(), 2);
        assert_eq!(
            responses[0].results_field().expect("accepted")["status"],
            "accepted"
        );
        assert_eq!(
            responses[1].results_field().expect("rejected")["status"],
            "rejected"
        );
        assert_eq!(
            responses[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
    }

    #[test]
    fn mission_sync_unknown_command_is_rejected_without_acceptance() {
        let mut core = RchCore::new();
        let command = command("mission.unknown", json!({}));

        let responses = core.handle_mission_sync_command(&command);

        assert_eq!(responses.len(), 1);
        assert_eq!(
            responses[0].results_field().expect("rejected")["status"],
            "rejected"
        );
        assert_eq!(
            responses[0].results_field().expect("rejected")["reason_code"],
            "unknown_command"
        );
    }

    #[test]
    fn rem_registry_commands_match_python_southbound_contract() {
        let mut core = RchCore::new();
        let unauthorized = core.handle_mission_sync_command(&command(
            "rem.registry.mode.set",
            json!({ "mode": "connected" }),
        ));
        assert_eq!(unauthorized.len(), 1);
        assert_eq!(unauthorized[0].content, "");
        assert_eq!(
            unauthorized[0].results_field().expect("rejected")["reason_code"],
            "unauthorized"
        );
        assert_eq!(
            unauthorized[0].results_field().expect("rejected")["reason"],
            "REM announce capabilities are required"
        );

        core.record_identity_announce(
            "ABCDEF",
            None,
            Some("REM Peer".to_string()),
            Some("identity".to_string()),
            vec!["r3akt".to_string(), "EmergencyMessages".to_string()],
        )
        .expect("record announce");

        let mode = core.handle_mission_sync_command(&command(
            "rem.registry.mode.set",
            json!({ "mode": "connected" }),
        ));
        assert_eq!(mode.len(), 2);
        assert!(mode.iter().all(|response| response.content.is_empty()));
        assert_eq!(
            mode[0].results_field().expect("accepted")["status"],
            "accepted"
        );
        assert_eq!(mode[1].results_field().expect("result")["status"], "result");
        assert_eq!(
            mode[1].results_field().expect("result")["result"]["identity"],
            "abcdef"
        );
        assert_eq!(
            mode[1].results_field().expect("result")["result"]["mode"],
            "connected"
        );
        assert_eq!(
            mode[1].results_field().expect("result")["result"]["effective_connected_mode"],
            true
        );
        assert_eq!(
            mode[1].event_field().expect("event")["event_type"],
            "rem.registry.mode.updated"
        );
        assert_eq!(
            mode[1].event_field().expect("event")["payload"]["mode"],
            "connected"
        );
        assert!(mode[1].event_field().expect("event")["source"]["rns_identity"].is_null());

        let peers =
            core.handle_mission_sync_command(&command("rem.registry.peers.list", json!({})));
        assert_eq!(peers.len(), 2);
        assert_eq!(
            peers[1].event_field().expect("event")["event_type"],
            "rem.registry.peers.listed"
        );
        let result = &peers[1].results_field().expect("result")["result"];
        assert_eq!(result["effective_connected_mode"], true);
        assert_eq!(result["items"][0]["identity"], "abcdef");
        assert_eq!(result["items"][0]["display_name"], "REM Peer");
        assert_eq!(
            result["items"][0]["announce_capabilities"],
            json!(["r3akt", "emergencymessages"])
        );
        assert_eq!(result["items"][0]["registered_mode"], "connected");
        assert!(
            core.audit_events()
                .iter()
                .any(|event| event.event_type == "rem.registry.peers.listed")
        );
    }

    #[test]
    fn rem_registry_rejects_unknown_and_invalid_commands_like_python() {
        let mut core = RchCore::new();
        let unsupported =
            core.handle_mission_sync_command(&command("rem.registry.unknown", json!({})));
        assert_eq!(unsupported.len(), 1);
        assert_eq!(unsupported[0].content, "");
        assert_eq!(
            unsupported[0].results_field().expect("rejected")["reason"],
            "Unsupported REM command 'rem.registry.unknown'"
        );

        core.record_identity_announce(
            "ABCDEF",
            None,
            None,
            Some("identity".to_string()),
            vec!["r3akt".to_string(), "EmergencyMessages".to_string()],
        )
        .expect("record announce");
        let invalid = core.handle_mission_sync_command(&command(
            "rem.registry.mode.set",
            json!({ "mode": "bogus" }),
        ));
        assert_eq!(invalid.len(), 2);
        assert_eq!(
            invalid[1].results_field().expect("rejected")["reason_code"],
            "invalid_payload"
        );
        assert_eq!(
            invalid[1].results_field().expect("rejected")["reason"],
            "invalid command payload: mode must be one of: autonomous, semi_autonomous, connected"
        );
    }

    #[test]
    fn mission_sync_authorization_can_require_python_rch_capabilities() {
        let mut core = RchCore::new();
        core.set_authorization_required(true);
        let create_command = command(
            "topic.create",
            json!({ "topic_path": "mission-auth", "topic_name": "Mission Auth" }),
        );

        let rejected = core.handle_mission_sync_command(&create_command);
        core.grant_identity_capability("abcdef", "topic.create");
        let accepted = core.handle_mission_sync_command(&create_command);

        assert_eq!(rejected.len(), 1);
        assert_eq!(
            rejected[0].results_field().expect("rejected")["status"],
            "rejected"
        );
        assert_eq!(
            rejected[0].results_field().expect("rejected")["reason_code"],
            "unauthorized"
        );
        assert_eq!(
            rejected[0].results_field().expect("rejected")["required_capabilities"][0],
            "topic.create"
        );
        assert_eq!(accepted.len(), 2);
        assert_eq!(
            accepted[0].results_field().expect("accepted")["status"],
            "accepted"
        );

        assert!(core.revoke_identity_capability("ABCDEF", "topic.create"));
        assert!(!core.has_identity_capability("abcdef", "topic.create"));
        let command_after_revoke = command(
            "topic.create",
            json!({ "topic_path": "mission-auth-revoked", "topic_name": "Mission Auth Revoked" }),
        );
        let rejected_after_revoke = core.handle_mission_sync_command(&command_after_revoke);
        assert_eq!(
            rejected_after_revoke[0].results_field().expect("rejected")["reason_code"],
            "unauthorized"
        );
    }

    #[test]
    fn mission_sync_authorization_accepts_mission_role_operations() {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "mission-role", "mission_name": "Mission Role" }),
        ));
        core.set_authorization_required(true);
        let log_command = command(
            "mission.registry.log_entry.upsert",
            json!({
                "mission_uid": "mission-role",
                "entry_uid": "log-1",
                "content": "role authorized update"
            }),
        );

        let rejected = core.handle_mission_sync_command(&log_command);
        core.assign_mission_access_role("mission-role", "identity", "ABCDEF", "MISSION_SUBSCRIBER")
            .expect("mission access assignment");
        let accepted = core.handle_mission_sync_command(&log_command);

        assert_eq!(rejected.len(), 1);
        assert_eq!(
            rejected[0].results_field().expect("rejected")["reason_code"],
            "unauthorized"
        );
        assert_eq!(accepted.len(), 2);
        assert_eq!(
            accepted[0].results_field().expect("accepted")["status"],
            "accepted"
        );
        assert_eq!(
            accepted[1].results_field().expect("result")["status"],
            "result"
        );
    }

    #[test]
    fn sqlite_snapshot_restores_topics_subscribers_messages_and_command_cache() {
        let send = command(
            "mission.message.send",
            json!({ "content": "hello", "topic_id": "mission-1" }),
        );
        let core = sqlite_snapshot_fixture_core(&send);

        let mut store = RchSqliteStore::in_memory().expect("sqlite");
        core.save_to_sqlite(&mut store).expect("save");
        assert_eq!(store.schema_version().expect("schema version"), "3");
        assert_sqlite_snapshot_counts(&store);

        let mut restored = RchCore::load_from_sqlite(&store)
            .expect("load")
            .expect("snapshot");
        assert_restored_sqlite_snapshot_shape(&restored);

        let replay = restored.handle_command(&send);
        assert_eq!(replay.result.status, CommandResultStatus::Accepted);
        assert!(replay.event.is_none());
        assert_eq!(restored.messages().len(), 1);
        assert!(restored.has_identity_capability("ABCDEF", "topic.create"));
    }

    #[test]
    fn sqlite_store_configures_busy_timeout_like_python_storage() {
        let store = RchSqliteStore::in_memory().expect("sqlite");

        let busy_timeout_ms: i64 = store
            .connection
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .expect("busy timeout");

        assert_eq!(
            busy_timeout_ms,
            i64::try_from(RCH_SQLITE_ADMIN_BUSY_TIMEOUT_MS).expect("busy timeout fits in i64")
        );
    }

    #[test]
    fn sqlite_store_records_ordered_migrations_and_safe_compaction_state() {
        let store = RchSqliteStore::in_memory().expect("sqlite");
        let migration_count: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM rch_schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("migration count");
        let stats = store.storage_stats().expect("storage stats");
        let report = store
            .compact_if_free_percent_exceeds(100.0, 100)
            .expect("no-op compaction");

        assert_eq!(migration_count, 3);

        let index_exists: bool = store
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = 'idx_rch_subscribers_topic_node')",
                [],
                |row| row.get(0),
            )
            .expect("subscriber topic index");
        assert!(index_exists);

        drop(store);
        assert!(stats.page_count > 0);
        assert!(!report.compacted);
        assert!(!report.requires_incremental_mode_conversion);
    }

    #[test]
    fn migration_file_applies_standalone_and_records_schema_version() {
        let connection = Connection::open_in_memory().expect("sqlite");
        connection
            .execute_batch(include_str!("../migrations/0001_rch_core_snapshot.sql"))
            .expect("migration");

        let version: String = connection
            .query_row(
                "SELECT setting_value FROM rch_settings WHERE setting_key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .expect("schema version");

        assert_eq!(version, "1");
    }

    #[test]
    fn sqlite_migration_adds_feed_publication_timestamp_to_legacy_table() {
        let db_path = std::env::temp_dir().join(format!(
            "r3akt-rch-core-feed-publication-migration-{}.db",
            Uuid::new_v4()
        ));
        {
            let connection = Connection::open(&db_path).expect("sqlite");
            connection
                .execute_batch(
                    "CREATE TABLE rch_checklist_feed_publications (
                        publication_uid TEXT PRIMARY KEY,
                        checklist_uid TEXT NOT NULL,
                        mission_feed_uid TEXT NOT NULL,
                        payload BLOB NOT NULL
                    );
                    INSERT INTO rch_checklist_feed_publications
                        (publication_uid, checklist_uid, mission_feed_uid, payload)
                    VALUES ('pub-1', 'checklist-1', 'feed-1', X'80');",
                )
                .expect("legacy feed table");
        }

        let store = RchSqliteStore::open(&db_path).expect("migrated store");
        assert_eq!(store.schema_version().expect("schema version"), "3");
        drop(store);

        let connection = Connection::open(&db_path).expect("sqlite");
        let mut statement = connection
            .prepare("PRAGMA table_info(rch_checklist_feed_publications)")
            .expect("table info");
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("column rows");
        let published_ts_ms: i64 = connection
            .query_row(
                "SELECT published_ts_ms FROM rch_checklist_feed_publications WHERE publication_uid = 'pub-1'",
                [],
                |row| row.get(0),
            )
            .expect("published timestamp");

        assert!(columns.iter().any(|column| column == "published_ts_ms"));
        assert_eq!(published_ts_ms, 0);
        drop(statement);
        drop(connection);
        let _ = std::fs::remove_file(db_path);
    }

    #[allow(clippy::too_many_lines)]
    fn sqlite_snapshot_fixture_core(send: &MissionCommandEnvelope) -> RchCore {
        let mut core = RchCore::new();
        core.handle_command(&command(
            "CreateTopic",
            json!({ "TopicID": "mission-1", "TopicPath": "mission-1", "TopicName": "Mission 1" }),
        ));
        core.handle_command(&command(
            "SubscribeTopic",
            json!({ "TopicID": "mission-1", "Destination": "FACEFEED" }),
        ));
        core.handle_command(send);
        core.handle_command(&command("mission.join", json!({})));
        core.handle_command(&command(
            "mission.marker.create",
            json!({ "name": "Smoke Marker", "lat": 45.0, "lon": -63.0 }),
        ));
        core.handle_command(&command(
            "mission.zone.create",
            json!({
                "name": "Smoke Zone",
                "points": [
                    { "lat": 45.0, "lon": -63.0 },
                    { "lat": 45.1, "lon": -63.1 },
                    { "lat": 45.1, "lon": -63.0 }
                ]
            }),
        ));
        core.handle_command(&command(
            "mission.registry.mission.upsert",
            json!({ "uid": "sqlite-mission", "mission_name": "SQLite Mission" }),
        ));
        let marker_id = core.markers()[0].object_destination_hash.clone();
        core.handle_command(&command(
            "mission.registry.mission.marker.link",
            json!({ "mission_uid": "sqlite-mission", "marker_id": marker_id }),
        ));
        core.handle_command(&command(
            "mission.registry.mission_change.upsert",
            json!({ "uid": "sqlite-change", "mission_uid": "sqlite-mission" }),
        ));
        core.handle_command(&command(
            "mission.registry.log_entry.upsert",
            json!({ "entry_uid": "sqlite-log", "mission_uid": "sqlite-mission", "content": "SQLite log" }),
        ));
        core.handle_command(&command(
            "mission.registry.team.upsert",
            json!({ "uid": "sqlite-team", "team_name": "SQLite Team", "mission_uid": "sqlite-mission" }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member.upsert",
            json!({
                "uid": "sqlite-member",
                "team_uid": "sqlite-team",
                "rns_identity": "peer-sqlite",
                "display_name": "Peer SQLite"
            }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member.client.link",
            json!({ "team_member_uid": "sqlite-member", "client_identity": "peer-client" }),
        ));
        core.handle_command(&command(
            "mission.registry.asset.upsert",
            json!({
                "asset_uid": "sqlite-asset",
                "team_member_uid": "sqlite-member",
                "name": "SQLite Asset",
                "asset_type": "POWER"
            }),
        ));
        core.handle_command(&command(
            "mission.registry.skill.upsert",
            json!({ "skill_uid": "sqlite-skill", "name": "SQLite Skill" }),
        ));
        core.handle_command(&command(
            "mission.registry.team_member_skill.upsert",
            json!({ "uid": "sqlite-member-skill", "team_member_rns_identity": "peer-sqlite", "skill_uid": "sqlite-skill", "level": 4 }),
        ));
        let now = utc_now_ms();
        core.checklist_tasks.insert(
            "sqlite-task".to_string(),
            ChecklistTaskRecord {
                task_uid: "sqlite-task".to_string(),
                checklist_uid: "sqlite-checklist".to_string(),
                number: 1,
                user_status: "PENDING".to_string(),
                task_status: "PENDING".to_string(),
                is_late: false,
                custom_status: None,
                due_relative_minutes: None,
                due_ts_ms: None,
                notes: None,
                row_background_color: None,
                line_break_enabled: false,
                completed_ts_ms: None,
                completed_by_team_member_rns_identity: None,
                legacy_value: None,
                created_ts_ms: now,
                updated_ts_ms: now,
            },
        );
        core.handle_command(&command(
            "mission.registry.task_skill_requirement.upsert",
            json!({ "uid": "sqlite-requirement", "task_uid": "sqlite-task", "skill_uid": "sqlite-skill", "minimum_level": 2 }),
        ));
        core.handle_command(&command(
            "mission.registry.assignment.upsert",
            json!({
                "assignment_uid": "sqlite-assignment",
                "mission_uid": "sqlite-mission",
                "task_uid": "sqlite-task",
                "team_member_rns_identity": "peer-sqlite",
                "assets": ["sqlite-asset"]
            }),
        ));
        core.set_authorization_required(true);
        core.grant_identity_capability("abcdef", "topic.create");
        core
    }

    fn assert_sqlite_snapshot_counts(store: &RchSqliteStore) {
        assert_eq!(store.row_count("rch_topics").expect("topics"), 1);
        assert_eq!(store.row_count("rch_subscribers").expect("subscribers"), 1);
        assert_eq!(store.row_count("rch_messages").expect("messages"), 1);
        assert_eq!(store.row_count("rch_clients").expect("clients"), 1);
        assert_eq!(store.row_count("rch_audit_events").expect("events"), 20);
        assert_eq!(store.row_count("rch_markers").expect("markers"), 1);
        assert_eq!(store.row_count("rch_zones").expect("zones"), 1);
        assert_eq!(store.row_count("rch_missions").expect("missions"), 1);
        assert_eq!(store.row_count("rch_mission_changes").expect("changes"), 4);
        assert_eq!(store.row_count("rch_log_entries").expect("log entries"), 1);
        assert_eq!(store.row_count("rch_teams").expect("teams"), 1);
        assert_eq!(store.row_count("rch_mission_team_links").expect("links"), 1);
        assert_eq!(
            store
                .row_count("rch_mission_marker_links")
                .expect("marker links"),
            1
        );
        assert_eq!(store.row_count("rch_team_members").expect("members"), 1);
        assert_eq!(
            store
                .row_count("rch_team_member_client_links")
                .expect("member links"),
            1
        );
        assert_eq!(store.row_count("rch_assets").expect("assets"), 1);
        assert_eq!(store.row_count("rch_skills").expect("skills"), 1);
        assert_eq!(
            store
                .row_count("rch_team_member_skills")
                .expect("member skills"),
            1
        );
        assert_eq!(
            store
                .row_count("rch_task_skill_requirements")
                .expect("requirements"),
            1
        );
        assert_eq!(store.row_count("rch_assignments").expect("assignments"), 1);
        assert_eq!(
            store
                .row_count("rch_assignment_asset_links")
                .expect("assignment assets"),
            1
        );
        assert_eq!(store.row_count("rch_command_results").expect("results"), 18);
        assert_eq!(
            store.row_count("rch_identity_capabilities").expect("caps"),
            1
        );
        assert_eq!(store.row_count("rch_settings").expect("settings"), 2);
    }

    fn assert_restored_sqlite_snapshot_shape(restored: &RchCore) {
        assert_eq!(restored.topics().len(), 1);
        assert_eq!(restored.subscribers("mission-1").len(), 1);
        assert_eq!(restored.messages().len(), 1);
        assert_eq!(restored.clients().len(), 1);
        assert_eq!(restored.audit_events().len(), 20);
        assert_eq!(restored.markers().len(), 1);
        assert_eq!(restored.zones().len(), 1);
        assert_eq!(restored.missions().len(), 1);
        assert_eq!(
            restored.mission_value(&restored.missions()[0])["markers"][0].as_str(),
            Some(restored.markers()[0].object_destination_hash.as_str())
        );
        assert_eq!(restored.mission_changes().len(), 4);
        assert_eq!(restored.log_entries().len(), 1);
        assert_eq!(restored.teams().len(), 1);
        assert_eq!(restored.team_members().len(), 1);
        assert_eq!(restored.assets().len(), 1);
        assert_eq!(restored.skills().len(), 1);
        assert_eq!(restored.team_member_skills().len(), 1);
        assert_eq!(restored.task_skill_requirements().len(), 1);
        assert_eq!(restored.assignments().len(), 1);
    }

    include!("topic_subscription_corrections_tests.rs");
}
