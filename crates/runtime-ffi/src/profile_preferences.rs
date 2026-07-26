use nmp::RelayUrl;
use nmp_native_runtime_store::{
    MAXIMUM_PROFILE_RELAYS_PER_LANE, PermissionDefaultPreference, ProfilePreferences,
};

use crate::{RuntimePermissionDefault, RuntimeRefusal};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeProfilePreferences {
    pub indexer_relays: Vec<String>,
    pub app_relays: Vec<String>,
    pub permission_default: RuntimePermissionDefault,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeProfilePreferencesUpdate {
    pub applied: bool,
    pub restart_required: bool,
    pub preferences: Option<RuntimeProfilePreferences>,
    pub refusal: Option<RuntimeRefusal>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RuntimeStorageSnapshot {
    pub nmp_cache_bytes: u64,
    pub app_data_bytes: u64,
    pub total_bytes: u64,
    pub incomplete: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeStorageResetResult {
    pub reset: bool,
    pub refusal: Option<RuntimeRefusal>,
}

pub(crate) fn validate_profile_preferences(
    preferences: RuntimeProfilePreferences,
) -> Result<ProfilePreferences, String> {
    validate_relay_lane("indexer", &preferences.indexer_relays, true)?;
    validate_relay_lane("app", &preferences.app_relays, true)?;
    store_preferences(preferences)
}

pub(crate) fn validate_configured_profile_preferences(
    preferences: RuntimeProfilePreferences,
) -> Result<ProfilePreferences, String> {
    validate_relay_lane("indexer", &preferences.indexer_relays, false)?;
    validate_relay_lane("app", &preferences.app_relays, false)?;
    store_preferences(preferences)
}

fn store_preferences(preferences: RuntimeProfilePreferences) -> Result<ProfilePreferences, String> {
    ProfilePreferences::new(
        preferences.indexer_relays,
        preferences.app_relays,
        stored_permission_default(preferences.permission_default),
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn project_profile_preferences(
    preferences: &ProfilePreferences,
) -> RuntimeProfilePreferences {
    RuntimeProfilePreferences {
        indexer_relays: preferences.indexer_relays.clone(),
        app_relays: preferences.app_relays.clone(),
        permission_default: match preferences.permission_default {
            PermissionDefaultPreference::AskEveryTime => RuntimePermissionDefault::AskEveryTime,
            PermissionDefaultPreference::AllowSession => RuntimePermissionDefault::AllowSession,
            PermissionDefaultPreference::AllowExactBuild => {
                RuntimePermissionDefault::AllowExactBuild
            }
        },
    }
}

pub(crate) fn stored_permission_default(
    preference: RuntimePermissionDefault,
) -> PermissionDefaultPreference {
    match preference {
        RuntimePermissionDefault::AskEveryTime => PermissionDefaultPreference::AskEveryTime,
        RuntimePermissionDefault::AllowSession => PermissionDefaultPreference::AllowSession,
        RuntimePermissionDefault::AllowExactBuild => PermissionDefaultPreference::AllowExactBuild,
    }
}

fn validate_relay_lane(
    lane: &str,
    relays: &[String],
    require_non_empty: bool,
) -> Result<(), String> {
    if require_non_empty && relays.is_empty() {
        return Err(format!(
            "{lane} relays must contain at least one secure relay"
        ));
    }
    if relays.len() > MAXIMUM_PROFILE_RELAYS_PER_LANE {
        return Err(format!(
            "{lane} relays has {} entries; the maximum is {MAXIMUM_PROFILE_RELAYS_PER_LANE}",
            relays.len()
        ));
    }
    for relay in relays {
        if !relay.starts_with("wss://") || relay.contains('@') || RelayUrl::parse(relay).is_err() {
            return Err(format!(
                "{lane} relays must use valid wss:// addresses without credentials"
            ));
        }
    }
    Ok(())
}
