# REM Southbound Interface

RCH emits REM-capable southbound messages as LXMF `FIELD_COMMANDS` (`0x09`)
payloads. Each command entry uses the reduced REM signature only:

```json
{
  "command_type": "checklist.task.status.set",
  "args": {
    "checklist_uid": "checklist-1",
    "task_uid": "task-1",
    "user_status": "COMPLETE"
  }
}
```

The southbound wire payload does not include legacy envelope metadata such as
`command_id`, `correlation_id`, `source`, `timestamp`, or `topics`. RCH also
does not emit an alternate encoded compatibility payload such as
`_lxmf_fields_msgpack_b64`. The Reticulum message body for REM commands is a
small placeholder (`cmd`), while the command data lives in `FIELD_COMMANDS`.
When the Rust server uses the LXMF-rs ZeroMQ SDK path, REM `auto` delivery is
submitted to the daemon as a propagated send so the reduced command payload is
queued through the selected propagation node instead of attempting a direct
link-first send.

## Common Shape

Every REM command is carried as:

```json
{
  "9": [
    {
      "command_type": "<operation>",
      "args": {}
    }
  ]
}
```

`args` contains only the operation data required by the receiving REM command.
Null values are omitted. IDs are sent as plain trimmed strings.

`checklist.upload` adds a native `snapshot` object beside `args`:

```json
{
  "command_type": "checklist.upload",
  "args": {
    "checklist_uid": "checklist-1",
    "mission_uid": "mission-1"
  },
  "snapshot": {}
}
```

## Command Inventory

Checklist commands:

- `checklist.create.online`
- `checklist.upload`
- `checklist.task.row.add`
- `checklist.task.row.delete`
- `checklist.task.row.style.set`
- `checklist.task.cell.set`
- `checklist.task.status.set`

Mission/log commands:

- `mission.registry.log_entry.upsert`

EAM commands:

- `mission.registry.eam.upsert`
- `mission.registry.eam.delete`

Telemetry and mission-content commands:

- `mission.registry.telemetry.upsert`
- `mission.registry.mission.marker.link`
- `mission.registry.mission.marker.unlink`
- `mission.registry.mission.zone.link`
- `mission.registry.mission.zone.unlink`

Some mission-content operations are ahead of current REM UI/runtime support.
They still use the same reduced signature so the southbound protocol has one
forward-compatible shape.

## Migration Note

The old verbose southbound command envelope is intentionally unsupported for REM
fanout. Receivers should not expect RCH to provide command IDs, correlations,
source metadata, timestamps, topics, `snapshot_json`, or a second encoded field
copy for REM southbound commands.

## Team Peer Directory

REM clients operating in semi-autonomous mode request their authoritative peer
directory over LXMF with `rem.registry.team_peers.list`. The request uses the
normal mission-style command envelope in `FIELD_COMMANDS` (`0x09`):

```json
{
  "command_id": "hub-directory-123",
  "command_type": "rem.registry.team_peers.list",
  "timestamp": "2026-07-16T12:00:00Z",
  "source": { "rns_identity": "<requesting REM identity>" },
  "args": {}
}
```

RCH requires the caller to have REM announce capabilities and to be linked to a
TEAM member. Version 2 returns canonical shared TEAM records, caller
memberships, and durable REM-member destinations, including members whose last
validated announce is now offline. Non-REM, unverified, banned, blackholed, and
self entries are excluded. Primary TEAM member identities and explicitly linked
client identities are both eligible. The legacy `items` array remains recent
and active only so older REM clients retain their existing behavior.

The terminal `FIELD_RESULTS` (`0x0a`) result payload is:

```json
{
  "schema_version": 2,
  "scope": "shared_teams",
  "effective_connected_mode": false,
  "teams": [
    {
      "uid": "d6b6e188b910d6bdd24d04b7a7ec5444",
      "color": "YELLOW",
      "team_name": "Yellow"
    }
  ],
  "caller_memberships": [
    {
      "team_uid": "d6b6e188b910d6bdd24d04b7a7ec5444",
      "team_member_uid": "<caller TEAM member UID>"
    }
  ],
  "members": [
    {
      "team_uid": "d6b6e188b910d6bdd24d04b7a7ec5444",
      "team_member_uid": "<peer TEAM member UID>",
      "identity": "<TEAM member identity>",
      "destination_hash": "<last validated lxmf.delivery destination>",
      "display_name": "Field Phone",
      "announce_capabilities": ["r3akt", "emergencymessages", "telemetry"],
      "client_type": "rem",
      "registered_mode": "semi_autonomous",
      "last_seen": "2026-07-16T12:00:00Z",
      "status": "offline"
    }
  ],
  "items": [
    {
      "identity": "<TEAM member identity>",
      "destination_hash": "<lxmf.delivery destination>",
      "display_name": "Field Phone",
      "announce_capabilities": ["r3akt", "emergencymessages", "telemetry"],
      "client_type": "rem",
      "registered_mode": "semi_autonomous",
      "last_seen": "2026-07-16T12:00:00Z",
      "status": "active"
    }
  ]
}
```

REM team-scoped traffic carries the canonical TEAM UID in LXMF `FIELD_GROUP`
(`0x0B`). RCH rejects unknown TEAM UIDs and callers that do not belong to the
requested TEAM. In Connected mode, RCH fanout is restricted to the validated
TEAM's durable REM destinations. Commands without `FIELD_GROUP` remain accepted
for compatibility with older REM clients.

`rem.registry.peers.list` remains available as the legacy unscoped REM registry
command, and `GET /api/rem/peers` remains the operator-facing HTTP registry.
