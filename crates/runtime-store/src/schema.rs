use rusqlite::{Connection, OptionalExtension};

use crate::StoreError;

const SCHEMA_VERSION: i64 = 4;

pub(crate) fn migrate(connection: &Connection) -> Result<(), StoreError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS runtime_schema (
            version INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS installations (
            author TEXT NOT NULL,
            d_tag TEXT NOT NULL,
            aggregate_hash TEXT NOT NULL,
            title TEXT NOT NULL,
            manifest_metadata TEXT NOT NULL,
            capability_requests TEXT NOT NULL DEFAULT '[]',
            PRIMARY KEY(author, d_tag, aggregate_hash)
        );
        CREATE TABLE IF NOT EXISTS grants (
            author TEXT NOT NULL,
            d_tag TEXT NOT NULL,
            aggregate_hash TEXT NOT NULL,
            capability TEXT NOT NULL,
            decision TEXT NOT NULL,
            PRIMARY KEY(author, d_tag, aggregate_hash, capability)
        );
        CREATE TABLE IF NOT EXISTS component_kv (
            author TEXT NOT NULL,
            d_tag TEXT NOT NULL,
            aggregate_hash TEXT NOT NULL,
            domain TEXT NOT NULL,
            key TEXT NOT NULL,
            value BLOB NOT NULL,
            PRIMARY KEY(author, d_tag, aggregate_hash, domain, key)
        );
        CREATE TABLE IF NOT EXISTS workspaces (
            id TEXT PRIMARY KEY NOT NULL,
            definition TEXT NOT NULL,
            retained_receipts TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS activity (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            author TEXT NOT NULL,
            d_tag TEXT NOT NULL,
            aggregate_hash TEXT NOT NULL,
            category TEXT NOT NULL,
            operation TEXT NOT NULL,
            outcome TEXT NOT NULL,
            occurred_at_millis INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS profile_preferences (
            id INTEGER PRIMARY KEY NOT NULL CHECK(id = 1),
            indexer_relays TEXT NOT NULL,
            app_relays TEXT NOT NULL,
            permission_default TEXT NOT NULL
        );",
    )?;
    let existing: Option<i64> = connection
        .query_row("SELECT version FROM runtime_schema LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    match existing {
        None => {
            create_workspace_assignments(connection)?;
            add_capability_requests_column(connection)?;
            create_profile_preferences(connection)?;
            connection.execute(
                "INSERT INTO runtime_schema(version) VALUES (?1)",
                [SCHEMA_VERSION],
            )?;
        }
        Some(1) => {
            create_workspace_assignments(connection)?;
            add_capability_requests_column(connection)?;
            create_profile_preferences(connection)?;
            connection.execute("UPDATE runtime_schema SET version = ?1", [SCHEMA_VERSION])?;
        }
        Some(2) => {
            add_capability_requests_column(connection)?;
            create_profile_preferences(connection)?;
            connection.execute("UPDATE runtime_schema SET version = ?1", [SCHEMA_VERSION])?;
        }
        Some(3) => {
            create_profile_preferences(connection)?;
            connection.execute("UPDATE runtime_schema SET version = ?1", [SCHEMA_VERSION])?;
        }
        Some(SCHEMA_VERSION) => {}
        Some(version) => return Err(StoreError::UnsupportedSchema(version)),
    }
    Ok(())
}

fn create_profile_preferences(connection: &Connection) -> Result<(), StoreError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS profile_preferences (
            id INTEGER PRIMARY KEY NOT NULL CHECK(id = 1),
            indexer_relays TEXT NOT NULL,
            app_relays TEXT NOT NULL,
            permission_default TEXT NOT NULL
        );",
    )?;
    Ok(())
}

fn add_capability_requests_column(connection: &Connection) -> Result<(), StoreError> {
    let present: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM pragma_table_info('installations')
            WHERE name = 'capability_requests'
        )",
        [],
        |row| row.get(0),
    )?;
    if !present {
        connection.execute(
            "ALTER TABLE installations
             ADD COLUMN capability_requests TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    Ok(())
}

fn create_workspace_assignments(connection: &Connection) -> Result<(), StoreError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS workspace_assignments (
            workspace_id TEXT NOT NULL,
            author TEXT NOT NULL,
            d_tag TEXT NOT NULL,
            aggregate_hash TEXT NOT NULL,
            PRIMARY KEY(workspace_id, author, d_tag, aggregate_hash),
            FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
            FOREIGN KEY(author, d_tag, aggregate_hash)
                REFERENCES installations(author, d_tag, aggregate_hash) ON DELETE CASCADE
        );",
    )?;
    Ok(())
}
