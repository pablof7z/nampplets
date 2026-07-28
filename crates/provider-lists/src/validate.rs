use std::{collections::BTreeSet, sync::Arc};

use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    ListEntry, ListItemTag, ListMutation, ListSelector, ListsProviderLimits, SupportedList,
    catalog::supported_list,
};

/// A refusal the napplet sees as `ok: false` with an exact reason.
///
/// These are protocol outcomes, not transport faults: the request was
/// well-formed enough to answer, and the answer is no.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ListRefusal {
    #[error("list must be an object with a numeric kind")]
    MalformedSelector,
    #[error("this runtime does not service list kind {0}")]
    UnsupportedKind(u16),
    #[error("list kind {0} is addressed by a d identifier, which is missing")]
    IdentifierRequired(u16),
    #[error("list kind {0} takes no d identifier")]
    IdentifierNotAllowed(u16),
    #[error("identifier must be 1..={0} bytes")]
    IdentifierBounds(usize),
    #[error("items must be a non-empty array of {{type, value}} objects")]
    MalformedItems,
    #[error("items must hold 1..={0} entries")]
    ItemBounds(usize),
    #[error("list kind {kind} does not accept {tag} items")]
    ItemTypeRejected { kind: u16, tag: ListItemTag },
    #[error("a {tag} value must be {expectation}")]
    ItemValueRejected {
        tag: ListItemTag,
        expectation: Arc<str>,
    },
    #[error("the same item appears more than once in one request")]
    DuplicateItem,
    #[error("the resulting list would exceed its {0}-entry bound")]
    ListFull(usize),
    #[error("no account is connected, so there is no list to change")]
    NoAccount,
}

pub(crate) fn parse_selector(
    list: Option<&Value>,
    limits: ListsProviderLimits,
) -> Result<(&'static SupportedList, ListSelector), ListRefusal> {
    let list = list
        .and_then(Value::as_object)
        .ok_or(ListRefusal::MalformedSelector)?;
    if list
        .keys()
        .any(|key| !["kind", "identifier"].contains(&key.as_str()))
    {
        return Err(ListRefusal::MalformedSelector);
    }
    let kind = list
        .get("kind")
        .and_then(Value::as_u64)
        .filter(|kind| *kind <= u64::from(u16::MAX))
        .ok_or(ListRefusal::MalformedSelector)? as u16;
    let supported = supported_list(kind).ok_or(ListRefusal::UnsupportedKind(kind))?;
    let identifier = match list.get("identifier") {
        None | Some(Value::Null) => None,
        Some(Value::String(identifier)) => Some(identifier.as_str()),
        Some(_) => return Err(ListRefusal::MalformedSelector),
    };
    match (supported.parameterized, identifier) {
        (true, None) => return Err(ListRefusal::IdentifierRequired(kind)),
        (false, Some(_)) => return Err(ListRefusal::IdentifierNotAllowed(kind)),
        _ => {}
    }
    if let Some(identifier) = identifier
        && (identifier.is_empty() || identifier.len() > limits.maximum_identifier_bytes)
    {
        return Err(ListRefusal::IdentifierBounds(
            limits.maximum_identifier_bytes,
        ));
    }
    Ok((
        supported,
        ListSelector {
            kind,
            identifier: identifier.map(Arc::from),
        },
    ))
}

pub(crate) fn parse_items(
    items: Option<&Value>,
    supported: &SupportedList,
    limits: ListsProviderLimits,
) -> Result<Vec<ListEntry>, ListRefusal> {
    let items = items
        .and_then(Value::as_array)
        .ok_or(ListRefusal::MalformedItems)?;
    if items.is_empty() || items.len() > limits.maximum_request_items {
        return Err(ListRefusal::ItemBounds(limits.maximum_request_items));
    }
    let mut parsed = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items {
        let entry = parse_item(item, supported, limits)?;
        if !seen.insert(entry.clone()) {
            return Err(ListRefusal::DuplicateItem);
        }
        parsed.push(entry);
    }
    Ok(parsed)
}

fn parse_item(
    item: &Value,
    supported: &SupportedList,
    limits: ListsProviderLimits,
) -> Result<ListEntry, ListRefusal> {
    let item = item.as_object().ok_or(ListRefusal::MalformedItems)?;
    if item.len() != 2 || !item.contains_key("type") || !item.contains_key("value") {
        return Err(ListRefusal::MalformedItems);
    }
    let tag = item
        .get("type")
        .and_then(Value::as_str)
        .and_then(ListItemTag::parse)
        .ok_or(ListRefusal::MalformedItems)?;
    if !supported.accepts(tag) {
        return Err(ListRefusal::ItemTypeRejected {
            kind: supported.kind,
            tag,
        });
    }
    let value = item
        .get("value")
        .and_then(Value::as_str)
        .ok_or(ListRefusal::MalformedItems)?;
    validate_value(tag, value, limits)?;
    Ok(ListEntry::new(tag, value))
}

fn validate_value(
    tag: ListItemTag,
    value: &str,
    limits: ListsProviderLimits,
) -> Result<(), ListRefusal> {
    let reject = |expectation: &str| ListRefusal::ItemValueRejected {
        tag,
        expectation: Arc::from(expectation),
    };
    match tag {
        ListItemTag::P | ListItemTag::E => {
            if !is_hex32(value) {
                return Err(reject("64 lowercase hex characters"));
            }
        }
        ListItemTag::T => {
            if value.is_empty()
                || value.len() > limits.maximum_value_bytes
                || value.chars().any(char::is_whitespace)
                || value.starts_with('#')
            {
                return Err(reject(
                    "a non-empty hashtag without whitespace or a leading #",
                ));
            }
        }
        ListItemTag::A => {
            if value.len() > limits.maximum_value_bytes || !is_address(value) {
                return Err(reject("kind:pubkey:identifier"));
            }
        }
    }
    Ok(())
}

/// Lowercase-only: NMP addresses events and keys by exact lowercase hex, so
/// accepting mixed case here would let the same entry appear twice.
fn is_hex32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_address(value: &str) -> bool {
    let mut parts = value.splitn(3, ':');
    let (Some(kind), Some(pubkey), Some(identifier)) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !kind.is_empty()
        && kind.len() <= 5
        && kind.bytes().all(|byte| byte.is_ascii_digit())
        && is_hex32(pubkey)
        && !identifier.contains(':')
}

/// Decides the exact result of adding `items` to `current`.
///
/// Order is preserved and additions append, so an unrelated reordering never
/// leaks out of a mutation.
pub(crate) fn apply_add(
    current: &[ListEntry],
    items: &[ListEntry],
    limits: ListsProviderLimits,
) -> Result<ListMutation, ListRefusal> {
    let present = current.iter().cloned().collect::<BTreeSet<_>>();
    let mut entries = current.to_vec();
    let mut changed = 0;
    let mut skipped = 0;
    for item in items {
        if present.contains(item) {
            skipped += 1;
        } else {
            entries.push(item.clone());
            changed += 1;
        }
    }
    if entries.len() > limits.maximum_list_entries {
        return Err(ListRefusal::ListFull(limits.maximum_list_entries));
    }
    Ok(ListMutation {
        entries,
        changed,
        skipped,
    })
}

/// Decides the exact result of removing `items` from `current`.
pub(crate) fn apply_remove(current: &[ListEntry], items: &[ListEntry]) -> ListMutation {
    let removing = items.iter().cloned().collect::<BTreeSet<_>>();
    let entries = current
        .iter()
        .filter(|entry| !removing.contains(*entry))
        .cloned()
        .collect::<Vec<_>>();
    let present = current.iter().cloned().collect::<BTreeSet<_>>();
    let changed = items.iter().filter(|item| present.contains(*item)).count();
    ListMutation {
        entries,
        changed,
        skipped: items.len() - changed,
    }
}

pub(crate) fn validate_limits(limits: ListsProviderLimits) -> bool {
    ![
        limits.maximum_sessions,
        limits.maximum_response_bytes,
        limits.maximum_draft_bytes,
        limits.maximum_correlation_id_bytes,
        limits.maximum_request_items,
        limits.maximum_list_entries,
        limits.maximum_identifier_bytes,
        limits.maximum_value_bytes,
    ]
    .contains(&0)
}

pub(crate) fn selector_value(selector: &ListSelector) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("kind".to_owned(), Value::from(selector.kind));
    if let Some(identifier) = &selector.identifier {
        map.insert(
            "identifier".to_owned(),
            Value::from(identifier.as_ref().to_owned()),
        );
    }
    map
}
