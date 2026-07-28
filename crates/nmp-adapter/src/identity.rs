//! Bounded public-identity projection over the supported NMP facade.
//!
//! Split out of `lib.rs` to keep that file shrinking: these helpers are one
//! cohesive concern (turning an NMP frame into the runtime's bounded identity
//! read) and nothing else in the adapter needs them.

use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
    sync::Arc,
};

use nmp::EngineError;
use nmp_native_runtime_core::{
    BoundedJson, PublicIdentity, PublicIdentityError, PublicIdentityQuery, PublicIdentityRead,
    PublicIdentityReadLimits,
};

use crate::{shortfall_json, source_status_name};

pub(crate) fn map_identity_engine_error(error: EngineError) -> PublicIdentityError {
    match error {
        EngineError::EngineClosed => PublicIdentityError::Closed,
        other => PublicIdentityError::Failed {
            reason: Arc::from(other.to_string()),
        },
    }
}

pub(crate) fn validate_identity_read_limits(
    limits: PublicIdentityReadLimits,
) -> Result<(), PublicIdentityError> {
    if limits.maximum_items == 0 || limits.maximum_sources == 0 || limits.maximum_frame_bytes == 0 {
        Err(PublicIdentityError::LimitExceeded)
    } else {
        Ok(())
    }
}

pub(crate) fn supported_identity_kind(query: &PublicIdentityQuery) -> Option<u16> {
    match query {
        PublicIdentityQuery::Relays => Some(10_002),
        PublicIdentityQuery::Profile => Some(0),
        PublicIdentityQuery::Follows => Some(3),
        PublicIdentityQuery::List { .. }
        | PublicIdentityQuery::Zaps
        | PublicIdentityQuery::Mutes
        | PublicIdentityQuery::Blocked
        | PublicIdentityQuery::Badges => None,
    }
}

pub(crate) fn identity_read_without_account(
    frozen_identity: PublicIdentity,
    query: &PublicIdentityQuery,
    limits: PublicIdentityReadLimits,
) -> Result<PublicIdentityRead, PublicIdentityError> {
    let value = match query {
        PublicIdentityQuery::Relays => serde_json::json!({}),
        PublicIdentityQuery::Profile => serde_json::Value::Null,
        PublicIdentityQuery::Follows => serde_json::json!([]),
        _ => {
            return Err(PublicIdentityError::QueryUnavailable {
                query: Arc::from(public_identity_query_name(query)),
            });
        }
    };
    bounded_identity_read(
        frozen_identity,
        value,
        serde_json::json!({
            "sources": [],
            "shortfall": [{"kind": "no_active_account"}],
        }),
        limits.maximum_frame_bytes,
    )
}

pub(crate) fn project_identity_frame(
    frozen_identity: PublicIdentity,
    query: &PublicIdentityQuery,
    frame: nmp::Frame,
    limits: PublicIdentityReadLimits,
) -> Result<PublicIdentityRead, PublicIdentityError> {
    let window = frame.window.ok_or_else(|| PublicIdentityError::Failed {
        reason: Arc::from("NMP identity observation returned an unbounded frame"),
    })?;
    if window.rows.len() > limits.maximum_items
        || frame.evidence.sources.len() > limits.maximum_sources
    {
        return Err(PublicIdentityError::LimitExceeded);
    }
    let frozen_pubkey = frozen_identity
        .account
        .as_ref()
        .ok_or(PublicIdentityError::InvalidSourceData)?
        .0
        .as_ref();
    if window
        .rows
        .iter()
        .any(|row| row.event.pubkey.to_string() != frozen_pubkey)
    {
        return Err(PublicIdentityError::InvalidSourceData);
    }
    let value = match query {
        PublicIdentityQuery::Relays => project_relay_list(&window.rows, limits.maximum_sources)?,
        PublicIdentityQuery::Profile => project_profile(&window.rows)?,
        PublicIdentityQuery::Follows => project_follows(&window.rows, limits.maximum_items)?,
        _ => {
            return Err(PublicIdentityError::QueryUnavailable {
                query: Arc::from(public_identity_query_name(query)),
            });
        }
    };
    let sources = frame
        .evidence
        .sources
        .iter()
        .map(|source| {
            serde_json::json!({
                "relay": source.relay.to_string(),
                "access": format!("{:?}", source.access),
                "reconciledThrough": source.reconciled_through.map(|value| value.as_secs()),
                "status": source_status_name(source.status),
            })
        })
        .collect::<Vec<_>>();
    let shortfall = frame
        .evidence
        .shortfall
        .iter()
        .map(shortfall_json)
        .collect::<Vec<_>>();
    bounded_identity_read(
        frozen_identity,
        value,
        serde_json::json!({
            "sources": sources,
            "shortfall": shortfall,
        }),
        limits.maximum_frame_bytes,
    )
}

pub(crate) fn project_profile(rows: &[nmp::Row]) -> Result<serde_json::Value, PublicIdentityError> {
    let Some(row) = rows.first() else {
        return Ok(serde_json::Value::Null);
    };
    if row.event.kind.as_u16() != 0 {
        return Err(PublicIdentityError::InvalidSourceData);
    }
    let raw: serde_json::Value = serde_json::from_str(&row.event.content)
        .map_err(|_| PublicIdentityError::InvalidSourceData)?;
    let object = raw
        .as_object()
        .ok_or(PublicIdentityError::InvalidSourceData)?;
    let mut profile = serde_json::Map::new();
    for (source, target) in [
        ("name", "name"),
        ("display_name", "displayName"),
        ("about", "about"),
        ("picture", "picture"),
        ("banner", "banner"),
        ("nip05", "nip05"),
        ("lud16", "lud16"),
        ("website", "website"),
    ] {
        if let Some(value) = object.get(source).and_then(serde_json::Value::as_str) {
            profile.insert(
                target.to_owned(),
                serde_json::Value::String(value.to_owned()),
            );
        }
    }
    Ok(serde_json::Value::Object(profile))
}

pub(crate) fn project_follows(
    rows: &[nmp::Row],
    maximum_items: usize,
) -> Result<serde_json::Value, PublicIdentityError> {
    let Some(row) = rows.first() else {
        return Ok(serde_json::json!([]));
    };
    if row.event.kind.as_u16() != 3 {
        return Err(PublicIdentityError::InvalidSourceData);
    }
    let mut follows = BTreeSet::new();
    for tag in row
        .event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().is_some_and(|kind| kind == "p"))
    {
        let Some(value) = tag.content() else {
            continue;
        };
        let Ok(pubkey) = nmp::PublicKey::from_str(value) else {
            continue;
        };
        follows.insert(pubkey.to_string());
        if follows.len() > maximum_items {
            return Err(PublicIdentityError::LimitExceeded);
        }
    }
    Ok(serde_json::json!(follows.into_iter().collect::<Vec<_>>()))
}

pub(crate) fn project_relay_list(
    rows: &[nmp::Row],
    maximum_sources: usize,
) -> Result<serde_json::Value, PublicIdentityError> {
    let Some(row) = rows.first() else {
        return Ok(serde_json::json!({}));
    };
    if row.event.kind.as_u16() != 10_002 {
        return Err(PublicIdentityError::InvalidSourceData);
    }
    let mut relays = BTreeMap::<String, (bool, bool)>::new();
    for tag in row
        .event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().is_some_and(|kind| kind == "r"))
    {
        let fields = tag.as_slice();
        let Some(raw_url) = fields.get(1) else {
            continue;
        };
        let Ok(relay) = nmp::RelayUrl::parse(raw_url) else {
            continue;
        };
        let permissions = match fields.get(2).map(String::as_str) {
            None => (true, true),
            Some("read") => (true, false),
            Some("write") => (false, true),
            Some(_) => continue,
        };
        let entry = relays.entry(relay.to_string()).or_insert((false, false));
        entry.0 |= permissions.0;
        entry.1 |= permissions.1;
        if relays.len() > maximum_sources {
            return Err(PublicIdentityError::LimitExceeded);
        }
    }
    Ok(serde_json::Value::Object(
        relays
            .into_iter()
            .map(|(relay, (read, write))| {
                (
                    relay,
                    serde_json::json!({
                        "read": read,
                        "write": write,
                    }),
                )
            })
            .collect(),
    ))
}

pub(crate) fn bounded_identity_read(
    frozen_identity: PublicIdentity,
    value: serde_json::Value,
    scoped_evidence: serde_json::Value,
    maximum_frame_bytes: usize,
) -> Result<PublicIdentityRead, PublicIdentityError> {
    let value_raw =
        serde_json::to_string(&value).map_err(|_| PublicIdentityError::InvalidSourceData)?;
    let evidence_raw = serde_json::to_string(&scoped_evidence)
        .map_err(|_| PublicIdentityError::InvalidSourceData)?;
    if value_raw.len().saturating_add(evidence_raw.len()) > maximum_frame_bytes {
        return Err(PublicIdentityError::LimitExceeded);
    }
    Ok(PublicIdentityRead {
        frozen_identity,
        value: BoundedJson::from_raw(value_raw, maximum_frame_bytes)
            .map_err(|_| PublicIdentityError::LimitExceeded)?,
        scoped_evidence: BoundedJson::from_raw(evidence_raw, maximum_frame_bytes)
            .map_err(|_| PublicIdentityError::LimitExceeded)?,
    })
}

pub(crate) fn public_identity_query_name(query: &PublicIdentityQuery) -> &'static str {
    match query {
        PublicIdentityQuery::Relays => "relays",
        PublicIdentityQuery::Profile => "profile",
        PublicIdentityQuery::Follows => "follows",
        PublicIdentityQuery::List { .. } => "list",
        PublicIdentityQuery::Zaps => "zaps",
        PublicIdentityQuery::Mutes => "mutes",
        PublicIdentityQuery::Blocked => "blocked",
        PublicIdentityQuery::Badges => "badges",
    }
}
