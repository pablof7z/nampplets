use std::collections::BTreeSet;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{RuntimeStore, StoreError};

pub const MAXIMUM_PROFILE_RELAYS_PER_LANE: usize = 4;
pub const MAXIMUM_PROFILE_RELAY_URL_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDefaultPreference {
    AskEveryTime,
    AllowSession,
    AllowExactBuild,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfilePreferences {
    pub indexer_relays: Vec<String>,
    pub app_relays: Vec<String>,
    pub permission_default: PermissionDefaultPreference,
}

impl ProfilePreferences {
    pub fn new(
        indexer_relays: Vec<String>,
        app_relays: Vec<String>,
        permission_default: PermissionDefaultPreference,
    ) -> Result<Self, StoreError> {
        validate_relay_lane("indexer", &indexer_relays)?;
        validate_relay_lane("app", &app_relays)?;
        Ok(Self {
            indexer_relays,
            app_relays,
            permission_default,
        })
    }
}

impl RuntimeStore {
    pub fn profile_preferences(&self) -> Result<Option<ProfilePreferences>, StoreError> {
        let connection = self.connection.lock();
        let encoded: Option<(String, String, String)> = connection
            .query_row(
                "SELECT indexer_relays, app_relays, permission_default
                 FROM profile_preferences WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((indexers, app_relays, permission_default)) = encoded else {
            return Ok(None);
        };
        let indexers = decode_relays(&indexers)?;
        let app_relays = decode_relays(&app_relays)?;
        ProfilePreferences::new(
            indexers,
            app_relays,
            decode_permission_default(&permission_default)?,
        )
        .map(Some)
    }

    pub fn save_profile_preferences(
        &self,
        preferences: &ProfilePreferences,
    ) -> Result<(), StoreError> {
        let validated = ProfilePreferences::new(
            preferences.indexer_relays.clone(),
            preferences.app_relays.clone(),
            preferences.permission_default,
        )?;
        let indexers = encode_relays(&validated.indexer_relays)?;
        let app_relays = encode_relays(&validated.app_relays)?;
        self.connection.lock().execute(
            "INSERT INTO profile_preferences(
                id, indexer_relays, app_relays, permission_default
             ) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                indexer_relays = excluded.indexer_relays,
                app_relays = excluded.app_relays,
                permission_default = excluded.permission_default",
            params![
                indexers,
                app_relays,
                encode_permission_default(validated.permission_default)
            ],
        )?;
        Ok(())
    }
}

fn validate_relay_lane(lane: &'static str, relays: &[String]) -> Result<(), StoreError> {
    if relays.len() > MAXIMUM_PROFILE_RELAYS_PER_LANE {
        return Err(StoreError::ProfileRelayCapacity {
            lane,
            actual: relays.len(),
            maximum: MAXIMUM_PROFILE_RELAYS_PER_LANE,
        });
    }
    let mut unique = BTreeSet::new();
    for relay in relays {
        if relay.is_empty()
            || relay
                .bytes()
                .any(|byte| byte == 0 || byte.is_ascii_control())
        {
            return Err(StoreError::InvalidProfileRelay { lane });
        }
        if relay.len() > MAXIMUM_PROFILE_RELAY_URL_BYTES {
            return Err(StoreError::ProfileRelayTooLarge {
                lane,
                actual: relay.len(),
                maximum: MAXIMUM_PROFILE_RELAY_URL_BYTES,
            });
        }
        if !unique.insert(relay) {
            return Err(StoreError::DuplicateProfileRelay { lane });
        }
    }
    Ok(())
}

fn encode_relays(relays: &[String]) -> Result<String, StoreError> {
    serde_json::to_string(relays).map_err(|error| StoreError::Corrupt(error.to_string()))
}

fn decode_relays(encoded: &str) -> Result<Vec<String>, StoreError> {
    serde_json::from_str(encoded).map_err(|error| StoreError::Corrupt(error.to_string()))
}

fn encode_permission_default(preference: PermissionDefaultPreference) -> &'static str {
    match preference {
        PermissionDefaultPreference::AskEveryTime => "ask_every_time",
        PermissionDefaultPreference::AllowSession => "allow_session",
        PermissionDefaultPreference::AllowExactBuild => "allow_exact_build",
    }
}

fn decode_permission_default(value: &str) -> Result<PermissionDefaultPreference, StoreError> {
    match value {
        "ask_every_time" => Ok(PermissionDefaultPreference::AskEveryTime),
        "allow_session" => Ok(PermissionDefaultPreference::AllowSession),
        "allow_exact_build" => Ok(PermissionDefaultPreference::AllowExactBuild),
        other => Err(StoreError::Corrupt(format!(
            "unknown profile permission default {other}"
        ))),
    }
}
