use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::events;

pub const CONNECTED_WINDOW: Duration = Duration::seconds(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Disabled,
    Waiting,
    Connected,
    Disconnected,
}

pub fn status(
    disabled_at: Option<DateTime<Utc>>,
    enrolled_at: Option<DateTime<Utc>>,
    last_seen_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Status {
    if disabled_at.is_some() {
        return Status::Disabled;
    }
    if enrolled_at.is_none() {
        return Status::Waiting;
    }
    match last_seen_at {
        Some(seen) if now - seen <= CONNECTED_WINDOW => Status::Connected,
        _ => Status::Disconnected,
    }
}

pub fn reported_at(at: Option<i64>, received: DateTime<Utc>) -> DateTime<Utc> {
    at.and_then(DateTime::from_timestamp_millis)
        .map_or(received, |at| at.min(received))
}

pub fn late(reported: DateTime<Utc>, received: DateTime<Utc>) -> Option<DateTime<Utc>> {
    (received - reported > CONNECTED_WINDOW).then_some(received)
}

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
        let ahead = at(2000).timestamp_millis();
        assert_eq!(reported_at(Some(ahead), received), received);
        assert_eq!(late(at(10), received), Some(received));
        assert_eq!(late(at(990), received), None);
    }

    #[test]
    fn statuses_and_their_boundaries() {
        assert_eq!(status(None, None, None, at(0)), Status::Waiting);
        assert_eq!(status(None, Some(at(0)), None, at(0)), Status::Disconnected);
        assert_eq!(
            status(None, Some(at(0)), Some(at(0)), at(90)),
            Status::Connected
        );
        assert_eq!(
            status(None, Some(at(0)), Some(at(0)), at(91)),
            Status::Disconnected
        );
        assert_eq!(
            status(Some(at(0)), Some(at(0)), Some(at(0)), at(1)),
            Status::Disabled
        );
    }
}
