use std::sync::Arc;

use nmp_native_nap_bridge::{ProviderCall, ProviderError, ProviderRequest};
use nmp_native_runtime_core::BoundedJson;
use serde_json::{Map, Value, json};

use crate::{
    DOMAIN, ListsProviderLimits, SUPPORTED_LISTS, validate::ListRefusal, write::ListsAction,
};

pub(crate) fn correlation_id(
    request: &ProviderRequest,
    limits: ListsProviderLimits,
) -> Result<Arc<str>, ProviderError> {
    let id = request
        .correlation_id
        .as_deref()
        .ok_or_else(|| invalid_payload(request, "id is required"))?;
    if id.is_empty() || id.len() > limits.maximum_correlation_id_bytes {
        return Err(invalid_payload(
            request,
            format!(
                "id must be 1..={} bytes",
                limits.maximum_correlation_id_bytes
            ),
        ));
    }
    Ok(Arc::from(id))
}

pub(crate) fn exact_payload<'a>(
    request: &'a ProviderRequest,
    allowed: &[&str],
) -> Result<&'a Map<String, Value>, ProviderError> {
    let payload = request
        .payload
        .as_object()
        .ok_or_else(|| invalid_payload(request, "payload must be a flat object"))?;
    if payload.len() != allowed.len() || payload.keys().any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(invalid_payload(
            request,
            format!("expected exactly these fields: {}", allowed.join(", ")),
        ));
    }
    Ok(payload)
}

pub(crate) fn invalid_payload(
    request: &ProviderRequest,
    reason: impl Into<Arc<str>>,
) -> ProviderError {
    ProviderError::InvalidPayload {
        domain: Arc::from(DOMAIN),
        action: Arc::clone(&request.action),
        reason: reason.into(),
    }
}

pub(crate) fn denied(request: &ProviderRequest, reason: impl Into<Arc<str>>) -> ProviderError {
    ProviderError::Denied {
        domain: Arc::from(DOMAIN),
        action: Arc::clone(&request.action),
        reason: reason.into(),
    }
}

pub(crate) fn failed(action: &str, reason: impl Into<Arc<str>>) -> ProviderError {
    ProviderError::Failed {
        domain: Arc::from(DOMAIN),
        action: Arc::from(action),
        reason: reason.into(),
    }
}

pub(crate) fn bounded(
    value: &Value,
    limits: ListsProviderLimits,
    action: &str,
) -> Result<BoundedJson, ProviderError> {
    BoundedJson::from_value(value, limits.maximum_response_bytes)
        .map_err(|_| failed(action, "lists response exceeds its configured byte limit"))
}

pub(crate) fn completed(
    value: &Value,
    limits: ListsProviderLimits,
    action: &str,
) -> Result<ProviderCall, ProviderError> {
    Ok(ProviderCall::completed(Some(bounded(
        value, limits, action,
    )?)))
}

/// The whole answer to `lists.supported`, projected from the pinned catalog.
pub(crate) fn supported_result(id: &str) -> Value {
    let lists = SUPPORTED_LISTS
        .iter()
        .map(|list| {
            json!({
                "kind": list.kind,
                "name": list.name,
                "parameterized": list.parameterized,
                "itemTypes": list
                    .item_types
                    .iter()
                    .map(|tag| tag.wire())
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "type": "lists.supported.result",
        "id": id,
        "lists": lists,
    })
}

/// A successful mutation result. `changed` is reported under the action's own
/// pinned field name (`added` / `removed`).
pub(crate) fn mutation_result(
    action: ListsAction,
    id: &str,
    changed: usize,
    skipped: usize,
) -> Value {
    mutation_envelope(action, id, true, changed, skipped, None)
}

/// A refused mutation. The counts stay present and zero so a napplet reading
/// them unconditionally cannot mistake a refusal for a partial success.
pub(crate) fn refusal_result(action: ListsAction, id: &str, refusal: &ListRefusal) -> Value {
    mutation_envelope(action, id, false, 0, 0, Some(refusal.to_string()))
}

fn mutation_envelope(
    action: ListsAction,
    id: &str,
    ok: bool,
    changed: usize,
    skipped: usize,
    error: Option<String>,
) -> Value {
    let mut envelope = Map::new();
    envelope.insert("type".to_owned(), Value::from(action.result_type()));
    envelope.insert("id".to_owned(), Value::from(id));
    envelope.insert("ok".to_owned(), Value::from(ok));
    envelope.insert(action.changed_field().to_owned(), Value::from(changed));
    envelope.insert("skipped".to_owned(), Value::from(skipped));
    if let Some(error) = error {
        envelope.insert("error".to_owned(), Value::from(error));
    }
    Value::Object(envelope)
}
