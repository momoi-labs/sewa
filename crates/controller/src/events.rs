//! The append-only history of each host.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

pub const HOST_CREATED: &str = "host.created";
pub const HOST_ENROLLED: &str = "host.enrolled";
pub const HOST_CONNECTED: &str = "host.connected";
pub const HOST_DISCONNECTED: &str = "host.disconnected";
pub const HOST_OFFLINE: &str = "host.offline";
pub const ROLE_CREATED: &str = "role.created";
pub const DRIFT_DETECTED: &str = "drift.detected";
pub const PASSKEY_REGISTERED: &str = "passkey.registered";
pub const PLAN_APPLIED: &str = "plan.applied";
pub const APPLY_FAILED: &str = "apply.failed";
pub const APPLY_REPLAN: &str = "apply.replan";
pub const DRIFT_REVERTED: &str = "drift.reverted";
pub const DRIFT_ADOPTED: &str = "drift.adopted";
pub const DRIFT_IGNORED: &str = "drift.ignored";
pub const ROLE_SAVED: &str = "role.saved";
pub const HOST_ROLES_SET: &str = "host.roles_set";
pub const HOST_ITEMS_SET: &str = "host.items_set";
pub const SYNC_CREATED: &str = "sync.created";
pub const SYNC_APPROVED: &str = "sync.approved";

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

/// Appends one event about a sync or a role, which may name a host.
pub async fn record_about(
    connection: &mut PgConnection,
    about: About,
    kind: &str,
    data: Value,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO events (host_id, sync_id, role_id, kind, data) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(about.host)
    .bind(about.sync)
    .bind(about.role)
    .bind(kind)
    .bind(data)
    .execute(connection)
    .await?;
    Ok(())
}

/// Appends one event that happened at `at` rather than now, such as a
/// report the agent kept while it could not reach the controller.
pub async fn record_at(
    connection: &mut PgConnection,
    about: About,
    kind: &str,
    data: Value,
    at: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO events (host_id, sync_id, role_id, kind, data, at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(about.host)
    .bind(about.sync)
    .bind(about.role)
    .bind(kind)
    .bind(data)
    .bind(at)
    .execute(connection)
    .await?;
    Ok(())
}

/// What an event is about.
#[derive(Debug, Default, Clone, Copy)]
pub struct About {
    pub host: Option<Uuid>,
    pub sync: Option<Uuid>,
    pub role: Option<Uuid>,
}
