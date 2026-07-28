//! NMP-backed implementation of the runtime's list-mutation port.
//!
//! The adapter never decides what a list *should* contain. It reads the
//! account's current replaceable event, projects its entries, and — once the
//! runtime has decided a new entry set — renders the replacement while
//! restoring everything it was not asked to change.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
    str::FromStr,
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};

use nmp::{Binding, Filter, IndexedTagName, LiveQuery, Window};
use nmp_native_runtime_core::{
    AccountRef, BoundedJson, Cancellation, ListEntry, ListItemTag, ListReadLimits, ListSelector,
    ListSnapshot, ListsDataError, ListsDataPlane,
};
use serde_json::{Value, json};

use crate::NmpDataPlane;

/// Tags the runtime addresses as list entries. Everything else on the event is
/// retained verbatim and handed back untouched.
const ENTRY_TAGS: &[ListItemTag] = &[
    ListItemTag::P,
    ListItemTag::E,
    ListItemTag::T,
    ListItemTag::A,
];

impl ListsDataPlane for NmpDataPlane {
    fn freeze_account(&self) -> Result<Option<AccountRef>, ListsDataError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(ListsDataError::Closed);
        }
        self.refresh_identity()
            .map(|identity| identity.account)
            .map_err(|_| ListsDataError::InvalidSourceData)
    }

    fn read_list(
        &self,
        account: &AccountRef,
        selector: &ListSelector,
        cancellation: &Cancellation,
        limits: ListReadLimits,
    ) -> Result<ListSnapshot, ListsDataError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(ListsDataError::Closed);
        }
        if cancellation.is_cancelled() {
            return Err(ListsDataError::Cancelled);
        }
        let author =
            nmp::PublicKey::from_str(&account.0).map_err(|_| ListsDataError::InvalidSourceData)?;
        let subscription = self
            .engine
            .observe(list_query(&author, selector)?, Some(replaceable_window()))
            .map_err(|error| {
                ListsDataError::Unaddressable(std::sync::Arc::from(error.to_string()))
            })?;
        if cancellation.is_cancelled() {
            subscription.cancel();
            return Err(ListsDataError::Cancelled);
        }
        let frame = subscription.recv().map_err(|_| {
            if self.closed.load(Ordering::Acquire) {
                ListsDataError::Closed
            } else {
                ListsDataError::InvalidSourceData
            }
        })?;
        if cancellation.is_cancelled() {
            return Err(ListsDataError::Cancelled);
        }
        let window = frame.window.ok_or(ListsDataError::InvalidSourceData)?;
        // A replaceable list has exactly one current event. NMP already
        // resolves replacement, so the newest row is the whole truth.
        let Some(row) = window.rows.first() else {
            return empty_snapshot(limits);
        };
        if row.event.pubkey != author || row.event.kind.as_u16() != selector.kind {
            return Err(ListsDataError::InvalidSourceData);
        }
        project_snapshot(&row.event, selector, limits)
    }

    fn draft_replacement(
        &self,
        account: &AccountRef,
        selector: &ListSelector,
        snapshot: &ListSnapshot,
        entries: &[ListEntry],
        maximum_draft_bytes: usize,
    ) -> Result<BoundedJson, ListsDataError> {
        let author =
            nmp::PublicKey::from_str(&account.0).map_err(|_| ListsDataError::InvalidSourceData)?;
        let retained = snapshot
            .retained
            .decode()
            .map_err(|_| ListsDataError::InvalidSourceData)?;
        let content = retained
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let originals = original_entry_tags(&retained);

        let mut tags = Vec::new();
        if let Some(identifier) = &selector.identifier {
            tags.push(vec!["d".to_owned(), identifier.as_ref().to_owned()]);
        }
        for tag in retained
            .get("otherTags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            tags.push(string_array(tag).ok_or(ListsDataError::InvalidSourceData)?);
        }
        for entry in entries {
            // An entry that was already published keeps its exact original
            // tag, so relay hints and petnames survive a mutation that never
            // mentioned them.
            match originals.get(&entry_key(entry)) {
                Some(original) => tags.push(original.clone()),
                None => tags.push(vec![
                    entry.tag.wire().to_owned(),
                    entry.value.as_ref().to_owned(),
                ]),
            }
        }

        let tags = tags
            .into_iter()
            .map(|values| nmp::Tag::parse(values).map_err(|_| ListsDataError::InvalidSourceData))
            .collect::<Result<Vec<_>, _>>()?;
        let unsigned = nmp::UnsignedEvent::new(
            author,
            nmp::Timestamp::from(now_seconds()),
            nmp::Kind::from(selector.kind),
            tags,
            content,
        );
        let value =
            serde_json::to_value(&unsigned).map_err(|_| ListsDataError::InvalidSourceData)?;
        BoundedJson::from_value(&value, maximum_draft_bytes)
            .map_err(|_| ListsDataError::DraftTooLarge)
    }
}

fn list_query(
    author: &nmp::PublicKey,
    selector: &ListSelector,
) -> Result<LiveQuery, ListsDataError> {
    let mut filter = Filter {
        kinds: Some(BTreeSet::from([selector.kind])),
        authors: Some(Binding::Literal(BTreeSet::from([author.to_string()]))),
        ..Filter::default()
    };
    if let Some(identifier) = &selector.identifier {
        let d = IndexedTagName::new('d').expect("d is an indexed NIP-01 tag");
        filter.tags.insert(
            d,
            Binding::Literal(BTreeSet::from([identifier.as_ref().to_owned()])),
        );
    }
    Ok(LiveQuery::from_filter(filter))
}

fn replaceable_window() -> Window {
    let one = NonZeroUsize::new(1).expect("1 is non-zero");
    Window::Expandable {
        initial: one,
        max: one,
    }
}

fn empty_snapshot(limits: ListReadLimits) -> Result<ListSnapshot, ListsDataError> {
    Ok(ListSnapshot {
        exists: false,
        entries: Vec::new(),
        retained: BoundedJson::from_value(
            &json!({"content": "", "otherTags": [], "entryTags": []}),
            limits.maximum_frame_bytes,
        )
        .map_err(|_| ListsDataError::DraftTooLarge)?,
    })
}

fn project_snapshot(
    event: &nmp::Event,
    selector: &ListSelector,
    limits: ListReadLimits,
) -> Result<ListSnapshot, ListsDataError> {
    let mut entries = Vec::new();
    let mut entry_tags = Vec::new();
    let mut other_tags = Vec::new();
    for tag in event.tags.iter() {
        let values = tag.as_slice();
        let Some(name) = values.first() else {
            continue;
        };
        // The `d` identifier is addressing, not content: it is rebuilt from
        // the selector rather than carried through as an opaque tag.
        if selector.identifier.is_some() && name == "d" {
            continue;
        }
        match ListItemTag::parse(name).filter(|tag| ENTRY_TAGS.contains(tag)) {
            Some(item) => {
                let Some(value) = values.get(1).filter(|value| !value.is_empty()) else {
                    other_tags.push(values.to_vec());
                    continue;
                };
                entries.push(ListEntry::new(item, value.as_str()));
                entry_tags.push(Value::from(values.to_vec()));
            }
            None => other_tags.push(values.to_vec()),
        }
        if entries.len() > limits.maximum_entries {
            return Err(ListsDataError::TooManyEntries);
        }
    }
    let retained = BoundedJson::from_value(
        &json!({
            "content": event.content,
            "otherTags": other_tags,
            "entryTags": entry_tags,
        }),
        limits.maximum_frame_bytes,
    )
    .map_err(|_| ListsDataError::TooManyEntries)?;
    Ok(ListSnapshot {
        exists: true,
        entries,
        retained,
    })
}

fn original_entry_tags(retained: &Value) -> BTreeMap<(String, String), Vec<String>> {
    retained
        .get("entryTags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(string_array)
        .filter_map(|values| {
            let name = values.first()?.clone();
            let value = values.get(1)?.clone();
            Some(((name, value), values))
        })
        .collect()
}

fn entry_key(entry: &ListEntry) -> (String, String) {
    (entry.tag.wire().to_owned(), entry.value.as_ref().to_owned())
}

fn string_array(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(ToOwned::to_owned))
        .collect()
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}
