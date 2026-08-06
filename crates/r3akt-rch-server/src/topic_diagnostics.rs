use super::{
    ApiError, AppState, BTreeMap, ConfigFileKind, HashMap, HashSet, Json, State, Value,
    apply_config_file, config_text_section_value, identity_announce_matches_destination, json,
    load_identity_announces_for_state, normalize_identity_key, normalize_topic_id,
};

pub(super) fn topic_subscription_diagnostics(state: &AppState) -> Result<Value, ApiError> {
    let topic_ids = state
        .topics
        .read()
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .keys()
        .filter_map(|topic_id| normalize_topic_id(Some(topic_id)))
        .collect::<HashSet<_>>();
    let subscribers = state
        .subscribers
        .read()
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .values()
        .cloned()
        .collect::<Vec<_>>();

    let topic_count = topic_ids.len();
    let subscriber_count = subscribers.len();
    let mut orphan_subscriber_count = 0_usize;
    let mut normalized_pairs = HashMap::<(String, String), usize>::new();
    let mut subscriber_destinations = Vec::with_capacity(subscribers.len());
    let mut missing_subscriber_identity_count = 0_usize;

    for subscriber in &subscribers {
        let normalized_topic_id = normalize_topic_id(Some(&subscriber.topic_id));
        if normalized_topic_id
            .as_ref()
            .is_none_or(|topic_id| !topic_ids.contains(topic_id))
        {
            orphan_subscriber_count = orphan_subscriber_count.saturating_add(1);
        }

        let Some(destination) = normalize_identity_key(&subscriber.destination) else {
            missing_subscriber_identity_count = missing_subscriber_identity_count.saturating_add(1);
            continue;
        };
        subscriber_destinations.push(destination.clone());
        if let Some(topic_id) = normalized_topic_id {
            *normalized_pairs.entry((destination, topic_id)).or_default() += 1;
        }
    }

    let duplicate_normalized_subscription_count = normalized_pairs
        .values()
        .map(|count| count.saturating_sub(1))
        .sum::<usize>();
    let announces = load_identity_announces_for_state(state)?;
    missing_subscriber_identity_count = missing_subscriber_identity_count.saturating_add(
        subscriber_destinations
            .iter()
            .filter(|destination| {
                !announces
                    .iter()
                    .any(|record| identity_announce_matches_destination(record, destination))
            })
            .count(),
    );
    let last_identity_announce_update_error = state
        .identity_announce_update_error
        .read()
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .clone();
    let identity_announce_update_supported = state.lxmf_zmq_data_plane.is_some();

    Ok(json!({
        "topic_count": topic_count,
        "subscriber_count": subscriber_count,
        "orphan_subscriber_count": orphan_subscriber_count,
        "duplicate_normalized_subscription_count": duplicate_normalized_subscription_count,
        "missing_subscriber_identity_count": missing_subscriber_identity_count,
        "identity_announce_update_supported": identity_announce_update_supported,
        "last_identity_announce_update_error": last_identity_announce_update_error,
        "count": topic_count,
        "subscribers": subscriber_count,
        "orphan_subscribers": orphan_subscriber_count,
        "duplicate_normalized_subscriptions": duplicate_normalized_subscription_count,
        "missing_subscriber_identities": missing_subscriber_identity_count,
    }))
}

pub(super) fn record_identity_announce_update_error(
    state: &AppState,
    error: Option<String>,
) -> Result<(), ApiError> {
    *state
        .identity_announce_update_error
        .write()
        .map_err(|poisoned| ApiError::Internal(poisoned.to_string()))? = error;
    Ok(())
}

pub(super) async fn apply_config_text(
    State(state): State<AppState>,
    body: String,
) -> Result<Json<Value>, ApiError> {
    let display_name = config_text_section_value(&body, "hub", &["display_name"])
        .unwrap_or_else(|| "RCH".to_string());
    let response = apply_config_file(state.config_path.clone(), body, ConfigFileKind::Hub)?;
    if let Some(data_plane) = state.lxmf_zmq_data_plane.clone() {
        let identity = match tokio::task::spawn_blocking(move || {
            data_plane.update_identity_announce(
                display_name,
                vec![
                    "r3akt".to_string(),
                    "emergencymessages".to_string(),
                    "telemetry".to_string(),
                ],
                BTreeMap::from([("service".to_string(), Value::String("rch".to_string()))]),
            )
        })
        .await
        {
            Ok(Ok(identity)) => identity,
            Ok(Err(error)) => {
                let message = format!("RCH identity announce update failed: {error}");
                record_identity_announce_update_error(&state, Some(message.clone()))?;
                return Err(ApiError::ServiceUnavailable(message));
            }
            Err(error) => {
                let message = format!("RCH identity announce update task failed: {error}");
                record_identity_announce_update_error(&state, Some(message.clone()))?;
                return Err(ApiError::ServiceUnavailable(message));
            }
        };
        let expected_source = state
            .reticulumd_source
            .as_deref()
            .map(String::as_str)
            .unwrap_or("");
        if identity
            .delivery_destination
            .as_deref()
            .is_none_or(|destination| !destination.eq_ignore_ascii_case(expected_source))
        {
            let message =
                "RCH identity update returned an unexpected delivery destination".to_string();
            record_identity_announce_update_error(&state, Some(message.clone()))?;
            return Err(ApiError::ServiceUnavailable(message));
        }
        record_identity_announce_update_error(&state, None)?;
    }
    Ok(response)
}
