//! The append-only history of each host.

use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

pub const HOST_CREATED: &str = "host.created";
pub const HOST_ENROLLED: &str = "host.enrolled";
pub const HOST_CONNECTED: &str = "host.connected";
pub const HOST_DISCONNECTED: &str = "host.disconnected";

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
