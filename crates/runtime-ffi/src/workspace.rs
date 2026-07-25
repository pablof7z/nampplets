//! Versioned native workspace persistence schema and its ingress invariants.

use std::{collections::BTreeSet, sync::Arc};

use nmp_native_runtime_app::WorkspaceView;
use nmp_native_runtime_core::{BoundedJson, WriteReceiptId};
use nmp_native_runtime_store::WorkspaceRecord;
use serde::{Deserialize, Serialize};

use crate::{
    MAXIMUM_WORKSPACE_FIELD_BYTES, MAXIMUM_WORKSPACE_JSON_BYTES, MAXIMUM_WORKSPACE_POINT_SIZE,
    MAXIMUM_WORKSPACE_RECEIPTS, MAXIMUM_WORKSPACE_SLOTS, RuntimeWorkspaceAxis,
    RuntimeWorkspaceDefinition, RuntimeWorkspaceRenderer, RuntimeWorkspaceRole,
    RuntimeWorkspaceSlot, WORKSPACE_SCHEMA_VERSION,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StoredWorkspaceAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StoredWorkspaceRole {
    Feed,
    Detail,
    Profile,
    Thread,
    Composer,
    MediaPlayer,
    ToolWindow,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StoredWorkspaceRenderer {
    Native,
    LegacyNapplet,
    Surface,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredWorkspaceSlotV1 {
    slot_id: String,
    role: StoredWorkspaceRole,
    renderer: StoredWorkspaceRenderer,
    handler_id: String,
    manifest_author: Option<String>,
    d_tag: Option<String>,
    aggregate_hash: Option<String>,
    binding_parameters: serde_json::Value,
    navigation: serde_json::Value,
    visible: bool,
    order: u16,
    size_points: u16,
    minimum_points: u16,
    maximum_points: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredWorkspaceV1 {
    schema_version: u16,
    axis: StoredWorkspaceAxis,
    slots: Vec<StoredWorkspaceSlotV1>,
    focused_slot_id: Option<String>,
    activity_drawer_visible: bool,
    preferences: serde_json::Value,
}

pub(crate) fn workspace_record_from_ffi(
    workspace: RuntimeWorkspaceDefinition,
) -> Result<WorkspaceRecord, String> {
    validate_workspace_name("workspace_id", &workspace.workspace_id)?;
    if workspace.schema_version != WORKSPACE_SCHEMA_VERSION {
        return Err(format!(
            "workspace schema version {} is unsupported; expected {WORKSPACE_SCHEMA_VERSION}",
            workspace.schema_version
        ));
    }
    if workspace.slots.is_empty() || workspace.slots.len() > MAXIMUM_WORKSPACE_SLOTS {
        return Err(format!(
            "workspace must contain 1..={MAXIMUM_WORKSPACE_SLOTS} slots"
        ));
    }
    if workspace.retained_receipt_ids.len() > MAXIMUM_WORKSPACE_RECEIPTS {
        return Err(format!(
            "workspace retains {} receipts; the maximum is {MAXIMUM_WORKSPACE_RECEIPTS}",
            workspace.retained_receipt_ids.len()
        ));
    }
    let preferences = parse_workspace_object("preferences_json", &workspace.preferences_json)?;
    let mut slot_ids = BTreeSet::new();
    let mut orders = BTreeSet::new();
    let mut stored_slots = Vec::with_capacity(workspace.slots.len());
    for slot in workspace.slots {
        validate_workspace_name("slot_id", &slot.slot_id)?;
        validate_workspace_name("handler_id", &slot.handler_id)?;
        if !slot_ids.insert(slot.slot_id.clone()) {
            return Err(format!("duplicate workspace slot id {:?}", slot.slot_id));
        }
        if !orders.insert(slot.order) {
            return Err(format!("duplicate workspace slot order {}", slot.order));
        }
        if slot.minimum_points == 0
            || slot.minimum_points > slot.size_points
            || slot.size_points > slot.maximum_points
            || slot.maximum_points > MAXIMUM_WORKSPACE_POINT_SIZE
        {
            return Err(format!(
                "slot {:?} size must satisfy 1 <= minimum <= size <= maximum <= {MAXIMUM_WORKSPACE_POINT_SIZE}",
                slot.slot_id
            ));
        }
        validate_workspace_handler(&slot)?;
        stored_slots.push(StoredWorkspaceSlotV1 {
            slot_id: slot.slot_id,
            role: stored_role(slot.role),
            renderer: stored_renderer(slot.renderer),
            handler_id: slot.handler_id,
            manifest_author: slot.manifest_author,
            d_tag: slot.d_tag,
            aggregate_hash: slot.aggregate_hash,
            binding_parameters: parse_workspace_object(
                "binding_parameters_json",
                &slot.binding_parameters_json,
            )?,
            navigation: parse_workspace_object("navigation_json", &slot.navigation_json)?,
            visible: slot.visible,
            order: slot.order,
            size_points: slot.size_points,
            minimum_points: slot.minimum_points,
            maximum_points: slot.maximum_points,
        });
    }
    if let Some(focused) = &workspace.focused_slot_id {
        validate_workspace_name("focused_slot_id", focused)?;
        if !stored_slots
            .iter()
            .any(|slot| slot.slot_id == *focused && slot.visible)
        {
            return Err("focused slot must name a visible workspace slot".to_owned());
        }
    }
    let mut receipt_ids = BTreeSet::new();
    let retained_receipts = workspace
        .retained_receipt_ids
        .into_iter()
        .map(|receipt_id| {
            validate_workspace_name("retained_receipt_id", &receipt_id)?;
            if !receipt_ids.insert(receipt_id.clone()) {
                return Err(format!("duplicate retained receipt id {receipt_id:?}"));
            }
            Ok(WriteReceiptId(Arc::from(receipt_id)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let stored = StoredWorkspaceV1 {
        schema_version: WORKSPACE_SCHEMA_VERSION,
        axis: stored_axis(workspace.axis),
        slots: stored_slots,
        focused_slot_id: workspace.focused_slot_id,
        activity_drawer_visible: workspace.activity_drawer_visible,
        preferences,
    };
    let value = serde_json::to_value(stored)
        .map_err(|error| format!("workspace serialization failed: {error}"))?;
    let definition = BoundedJson::from_value(&value, MAXIMUM_WORKSPACE_JSON_BYTES)
        .map_err(|error| error.to_string())?;
    Ok(WorkspaceRecord {
        id: Arc::from(workspace.workspace_id),
        definition,
        retained_receipts,
    })
}

pub(crate) fn workspace_from_view(
    workspace: &WorkspaceView,
) -> Result<RuntimeWorkspaceDefinition, String> {
    workspace_from_parts(
        workspace.id.as_ref(),
        &workspace.definition,
        &workspace.retained_receipts,
    )
}

pub(crate) fn workspace_from_record(
    workspace: &WorkspaceRecord,
) -> Result<RuntimeWorkspaceDefinition, String> {
    workspace_from_parts(
        workspace.id.as_ref(),
        &workspace.definition,
        &workspace.retained_receipts,
    )
}

fn workspace_from_parts(
    workspace_id: &str,
    definition: &BoundedJson,
    retained_receipts: &[WriteReceiptId],
) -> Result<RuntimeWorkspaceDefinition, String> {
    validate_workspace_name("workspace_id", workspace_id)?;
    let stored: StoredWorkspaceV1 = serde_json::from_str(definition.as_str())
        .map_err(|error| format!("workspace definition is malformed: {error}"))?;
    if stored.schema_version != WORKSPACE_SCHEMA_VERSION {
        return Err(format!(
            "workspace schema version {} is unsupported; expected {WORKSPACE_SCHEMA_VERSION}",
            stored.schema_version
        ));
    }
    let projected = RuntimeWorkspaceDefinition {
        schema_version: stored.schema_version,
        workspace_id: workspace_id.to_owned(),
        axis: ffi_axis(stored.axis),
        slots: stored
            .slots
            .into_iter()
            .map(|slot| RuntimeWorkspaceSlot {
                slot_id: slot.slot_id,
                role: ffi_role(slot.role),
                renderer: ffi_renderer(slot.renderer),
                handler_id: slot.handler_id,
                manifest_author: slot.manifest_author,
                d_tag: slot.d_tag,
                aggregate_hash: slot.aggregate_hash,
                binding_parameters_json: serde_json::to_string(&slot.binding_parameters)
                    .expect("serializing a parsed JSON value cannot fail"),
                navigation_json: serde_json::to_string(&slot.navigation)
                    .expect("serializing a parsed JSON value cannot fail"),
                visible: slot.visible,
                order: slot.order,
                size_points: slot.size_points,
                minimum_points: slot.minimum_points,
                maximum_points: slot.maximum_points,
            })
            .collect(),
        focused_slot_id: stored.focused_slot_id,
        activity_drawer_visible: stored.activity_drawer_visible,
        preferences_json: serde_json::to_string(&stored.preferences)
            .expect("serializing a parsed JSON value cannot fail"),
        retained_receipt_ids: retained_receipts
            .iter()
            .map(|receipt| receipt.0.to_string())
            .collect(),
    };
    // Apply every ingress invariant to durable data before returning it to a
    // native caller. This catches corrupt or pre-versioned rows atomically.
    let _ = workspace_record_from_ffi(projected.clone())?;
    Ok(projected)
}

fn validate_workspace_handler(slot: &RuntimeWorkspaceSlot) -> Result<(), String> {
    match slot.renderer {
        RuntimeWorkspaceRenderer::Native | RuntimeWorkspaceRenderer::Unavailable => {
            if slot.manifest_author.is_some()
                || slot.d_tag.is_some()
                || slot.aggregate_hash.is_some()
            {
                return Err(format!(
                    "slot {:?} native/unavailable handlers cannot carry a napplet principal",
                    slot.slot_id
                ));
            }
        }
        RuntimeWorkspaceRenderer::LegacyNapplet | RuntimeWorkspaceRenderer::Surface => {
            let author = slot
                .manifest_author
                .as_deref()
                .ok_or_else(|| format!("slot {:?} is missing manifest_author", slot.slot_id))?;
            let d_tag = slot
                .d_tag
                .as_deref()
                .ok_or_else(|| format!("slot {:?} is missing d_tag", slot.slot_id))?;
            let aggregate = slot
                .aggregate_hash
                .as_deref()
                .ok_or_else(|| format!("slot {:?} is missing aggregate_hash", slot.slot_id))?;
            validate_hex64("manifest_author", author)?;
            validate_workspace_name("d_tag", d_tag)?;
            validate_hex64("aggregate_hash", aggregate)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_workspace_name(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 256
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!(
            "{field} must be non-empty, control-free, and at most 256 bytes"
        ));
    }
    Ok(())
}

fn validate_hex64(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field} must be exactly 64 hexadecimal characters"));
    }
    Ok(())
}

fn parse_workspace_object(field: &str, raw: &str) -> Result<serde_json::Value, String> {
    if raw.len() > MAXIMUM_WORKSPACE_FIELD_BYTES {
        return Err(format!(
            "{field} is {} bytes; the maximum is {MAXIMUM_WORKSPACE_FIELD_BYTES}",
            raw.len()
        ));
    }
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("{field} is invalid JSON: {error}"))?;
    if !value.is_object() {
        return Err(format!("{field} must be a JSON object"));
    }
    Ok(value)
}

fn stored_axis(axis: RuntimeWorkspaceAxis) -> StoredWorkspaceAxis {
    match axis {
        RuntimeWorkspaceAxis::Horizontal => StoredWorkspaceAxis::Horizontal,
        RuntimeWorkspaceAxis::Vertical => StoredWorkspaceAxis::Vertical,
    }
}

fn ffi_axis(axis: StoredWorkspaceAxis) -> RuntimeWorkspaceAxis {
    match axis {
        StoredWorkspaceAxis::Horizontal => RuntimeWorkspaceAxis::Horizontal,
        StoredWorkspaceAxis::Vertical => RuntimeWorkspaceAxis::Vertical,
    }
}

fn stored_role(role: RuntimeWorkspaceRole) -> StoredWorkspaceRole {
    match role {
        RuntimeWorkspaceRole::Feed => StoredWorkspaceRole::Feed,
        RuntimeWorkspaceRole::Detail => StoredWorkspaceRole::Detail,
        RuntimeWorkspaceRole::Profile => StoredWorkspaceRole::Profile,
        RuntimeWorkspaceRole::Thread => StoredWorkspaceRole::Thread,
        RuntimeWorkspaceRole::Composer => StoredWorkspaceRole::Composer,
        RuntimeWorkspaceRole::MediaPlayer => StoredWorkspaceRole::MediaPlayer,
        RuntimeWorkspaceRole::ToolWindow => StoredWorkspaceRole::ToolWindow,
    }
}

fn ffi_role(role: StoredWorkspaceRole) -> RuntimeWorkspaceRole {
    match role {
        StoredWorkspaceRole::Feed => RuntimeWorkspaceRole::Feed,
        StoredWorkspaceRole::Detail => RuntimeWorkspaceRole::Detail,
        StoredWorkspaceRole::Profile => RuntimeWorkspaceRole::Profile,
        StoredWorkspaceRole::Thread => RuntimeWorkspaceRole::Thread,
        StoredWorkspaceRole::Composer => RuntimeWorkspaceRole::Composer,
        StoredWorkspaceRole::MediaPlayer => RuntimeWorkspaceRole::MediaPlayer,
        StoredWorkspaceRole::ToolWindow => RuntimeWorkspaceRole::ToolWindow,
    }
}

fn stored_renderer(renderer: RuntimeWorkspaceRenderer) -> StoredWorkspaceRenderer {
    match renderer {
        RuntimeWorkspaceRenderer::Native => StoredWorkspaceRenderer::Native,
        RuntimeWorkspaceRenderer::LegacyNapplet => StoredWorkspaceRenderer::LegacyNapplet,
        RuntimeWorkspaceRenderer::Surface => StoredWorkspaceRenderer::Surface,
        RuntimeWorkspaceRenderer::Unavailable => StoredWorkspaceRenderer::Unavailable,
    }
}

fn ffi_renderer(renderer: StoredWorkspaceRenderer) -> RuntimeWorkspaceRenderer {
    match renderer {
        StoredWorkspaceRenderer::Native => RuntimeWorkspaceRenderer::Native,
        StoredWorkspaceRenderer::LegacyNapplet => RuntimeWorkspaceRenderer::LegacyNapplet,
        StoredWorkspaceRenderer::Surface => RuntimeWorkspaceRenderer::Surface,
        StoredWorkspaceRenderer::Unavailable => RuntimeWorkspaceRenderer::Unavailable,
    }
}
