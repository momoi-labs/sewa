//! The append-only history of each host.

use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

pub const HOST_CREATED: &str = "host.created";
pub const HOST_ENROLLED: &str = "host.enrolled";
pub const HOST_CONNECTED: &str = "host.connected";
pub const HOST_DISCONNECTED: &str = "host.disconnected";
pub const ROLE_CREATED: &str = "role.created";
pub const CHANGE_CREATED: &str = "change.created";
pub const CHANGE_EDITED: &str = "change.edited";
pub const CHANGE_DISCARDED: &str = "change.discarded";
pub const DRIFT_DETECTED: &str = "drift.detected";

/// Appends one event, in the caller's transaction.
pub async fn record(
    connection: &mut PgConnection,
    host: Uuid,
    kind: &str,
    data: Value,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO events (host_id, kind, data) VALUES ($1, $2, $3)")
        .bind(host)
        .bind(kind)
        .bind(data)
        .execute(connection)
        .await?;
    Ok(())
}

/// Appends one event about a change or a role, which may name a host.
pub async fn record_about(
    connection: &mut PgConnection,
    about: About,
    kind: &str,
    data: Value,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO events (host_id, change_id, role_id, kind, data) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(about.host)
    .bind(about.change)
    .bind(about.role)
    .bind(kind)
    .bind(data)
    .execute(connection)
    .await?;
    Ok(())
}

/// What an event is about.
#[derive(Debug, Default, Clone, Copy)]
pub struct About {
    pub host: Option<Uuid>,
    pub change: Option<Uuid>,
    pub role: Option<Uuid>,
}
