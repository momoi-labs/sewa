//! The connection status of a host, computed when read, and the task that
//! records `host.disconnected` once a host goes quiet.

use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::events;

/// How long a host stays connected after its agent's last OpAMP message:
/// three of the agent's 30-second polls, as in pg-yard.
pub const CONNECTED_WINDOW: Duration = Duration::seconds(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The host never enrolled.
    Waiting,
    /// Its agent's last message arrived within [`CONNECTED_WINDOW`].
    Connected,
    /// It enrolled, but its agent has been quiet for longer than that, or
    /// never sent a message.
    Disconnected,
}

pub fn status(
    enrolled_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Status {
    if enrolled_at.is_none() {
        return Status::Waiting;
    }
    match last_seen_at {
        Some(seen) if now - seen <= CONNECTED_WINDOW => Status::Connected,
        _ => Status::Disconnected,
    }
}

/// The time an agent gave a report, in Unix milliseconds, as a time no
/// later than `received`. Without one, the report happened when received.
pub fn reported_at(at: Option<i64>, received: DateTime<Utc>) -> DateTime<Utc> {
    at.and_then(DateTime::from_timestamp_millis)
        .map_or(received, |at| at.min(received))
}

/// When a report arrived, if it arrived later than an agent sends one: it
/// waited in the agent's outbox while the controller was out of reach.
pub fn late(reported: DateTime<Utc>, received: DateTime<Utc>) -> Option<DateTime<Utc>> {
    (received - reported > CONNECTED_WINDOW).then_some(received)
}

/// Records `host.disconnected` for each host whose last event is
/// `host.connected` and whose agent went quiet past [`CONNECTED_WINDOW`].
/// Returns how many it recorded.
pub async fn sweep(pool: &PgPool) -> sqlx::Result<u64> {
    let recorded = sqlx::query(
        "WITH gone AS (
             UPDATE hosts SET connected = false
             WHERE connected AND last_seen_at < now() - make_interval(secs => $1)
             RETURNING id, last_seen_at
         )
         INSERT INTO events (host_id, kind, data)
         SELECT id, $2, jsonb_build_object('last_seen_at', last_seen_at) FROM gone",
    )
    .bind(CONNECTED_WINDOW.num_seconds() as f64)
    .bind(events::HOST_DISCONNECTED)
    .execute(pool)
    .await?;
    Ok(recorded.rows_affected())
}

/// Runs [`sweep`] every 10 seconds, forever.
pub async fn run_sweeper(pool: PgPool) {
    let mut every = tokio::time::interval(StdDuration::from_secs(10));
    loop {
        every.tick().await;
        if let Err(error) = sweep(&pool).await {
            tracing::warn!(%error, "cannot record disconnected hosts");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + seconds, 0).unwrap()
    }

    #[test]
    fn report_times_are_the_agents_but_never_later_than_received() {
        let received = at(1000);
        assert_eq!(reported_at(None, received), received);
        let millis = at(10).timestamp_millis();
        assert_eq!(reported_at(Some(millis), received), at(10));
        // A clock ahead of the controller's.
        let ahead = at(2000).timestamp_millis();
        assert_eq!(reported_at(Some(ahead), received), received);
        assert_eq!(late(at(10), received), Some(received));
        assert_eq!(late(at(990), received), None);
    }

    #[test]
    fn statuses_and_their_boundaries() {
        assert_eq!(status(None, None, at(0)), Status::Waiting);
        assert_eq!(status(Some(at(0)), None, at(0)), Status::Disconnected);
        assert_eq!(status(Some(at(0)), Some(at(0)), at(90)), Status::Connected);
        assert_eq!(
            status(Some(at(0)), Some(at(0)), at(91)),
            Status::Disconnected
        );
    }
}
