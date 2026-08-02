use r3akt_protocol::Command;
use serde_json::{Map, Value};

#[derive(Debug, PartialEq)]
pub(crate) enum DirectLxmfCommandResult {
    NoCommand,
    Valid(Command),
    Malformed(String),
}

pub(crate) fn direct_lxmf_command(fields: Option<&Map<String, Value>>) -> DirectLxmfCommandResult {
    let Some(fields) = fields else {
        return DirectLxmfCommandResult::NoCommand;
    };
    let Some(commands) = field_value(fields, &["9", "FIELD_COMMANDS"]) else {
        return DirectLxmfCommandResult::NoCommand;
    };
    let command = match commands {
        Value::Array(commands) => {
            let Some(command) = commands.first() else {
                return DirectLxmfCommandResult::Malformed(
                    "field 0x09 command list is empty".to_string(),
                );
            };
            command
        }
        command => command,
    };
    let Some(command) = command.as_object() else {
        return DirectLxmfCommandResult::Malformed(
            "first field 0x09 command entry must be an object".to_string(),
        );
    };
    let selector = if command.contains_key("command_type") {
        "command_type"
    } else if command.contains_key("Command") {
        "Command"
    } else if command.contains_key("0") {
        "0"
    } else {
        return DirectLxmfCommandResult::Malformed(
            "first field 0x09 command entry has no command_type, Command, or 0 selector"
                .to_string(),
        );
    };
    let Some(name) = command
        .get(selector)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return DirectLxmfCommandResult::Malformed(format!(
            "field 0x09 selector {selector:?} must be a non-empty string"
        ));
    };
    let correlation_id = command
        .get("command_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let args = if matches!(selector, "Command" | "0") {
        let mut args = Map::new();
        if let Some(Value::Object(explicit_args)) = command.get("args") {
            args.extend(explicit_args.clone());
        } else if let Some(explicit_args) = command.get("args") {
            args.insert("args".to_string(), explicit_args.clone());
        }
        for (key, value) in command {
            if !matches!(key.as_str(), "Command" | "0" | "command_id" | "args") {
                args.insert(key.clone(), value.clone());
            }
        }
        Value::Object(args)
    } else {
        command
            .get("args")
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}))
    };
    DirectLxmfCommandResult::Valid(Command {
        name: name.to_string(),
        args,
        correlation_id,
    })
}

fn field_value<'a>(fields: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| {
        fields.get(*key).or_else(|| {
            fields
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
                .map(|(_, value)| value)
        })
    })
}
