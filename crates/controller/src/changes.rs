//! `/api/changes`: the draft change and its edits, and each host's plan of
//! it. A change stays a draft until it is approved (not built yet) or
//! discarded. Every edit is a `change.edited` event.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::AppState;
use crate::desired::{self, Edit};
use crate::error::{ApiResult, Failure};
use crate::events::{self, About};
use crate::status::{Status, status};
use crate::whois::Identity;

#[derive(Debug, Serialize)]
pub struct Change {
    pub id: Uuid,
    /// `draft`, `approved`, or `discarded`.
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub closed_at: Option<DateTime<Utc>>,
    pub edit_count: i64,
}

type ChangeRow = (
    Uuid,
    String,
    DateTime<Utc>,
    String,
    Option<DateTime<Utc>>,
    i64,
);

/// A query of [`ChangeRow`]s: `SELECT <its columns> FROM changes c` and the
/// rest.
macro_rules! select_changes {
    ($rest:literal) => {
        concat!(
            "SELECT c.id, c.status, c.created_at, c.created_by, c.closed_at,
                    (SELECT count(*) FROM change_edits e WHERE e.change_id = c.id)
             FROM changes c ",
            $rest
        )
    };
}

fn change(row: ChangeRow) -> Change {
    let (id, status, created_at, created_by, closed_at, edit_count) = row;
    Change {
        id,
        status,
        created_at,
        created_by,
        closed_at,
        edit_count,
    }
}

fn change_id(id: &str) -> ApiResult<Uuid> {
    id.parse()
        .map_err(|_| Failure::new(StatusCode::NOT_FOUND, "not_found"))
}

/// `GET /api/changes`: every change, newest first.
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Change>>> {
    let rows: Vec<ChangeRow> = sqlx::query_as(select_changes!("ORDER BY c.created_at DESC"))
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(rows.into_iter().map(change).collect()))
}

/// `POST /api/changes/draft/edits`: adds one edit to the draft, and
/// creates the draft when there is none. A role or host the edit names
/// must exist.
pub async fn add_edit(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Json(edit): Json<Edit>,
) -> ApiResult<impl IntoResponse> {
    if !edit.is_valid() {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_item"));
    }
    let (role, host) = edit.targets();
    let mut transaction = state.pool.begin().await?;
    if let Some(role) = role {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM roles WHERE id = $1)")
            .bind(role)
            .fetch_one(&mut *transaction)
            .await?;
        if !exists {
            return Err(Failure::new(StatusCode::BAD_REQUEST, "unknown_role"));
        }
    }
    if let Some(host) = host {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM hosts WHERE id = $1)")
            .bind(host)
            .fetch_one(&mut *transaction)
            .await?;
        if !exists {
            return Err(Failure::new(StatusCode::BAD_REQUEST, "unknown_host"));
        }
    }
    let created: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO changes (created_by) VALUES ($1)
         ON CONFLICT ((true)) WHERE status = 'draft' DO NOTHING
         RETURNING id",
    )
    .bind(&identity.login)
    .fetch_optional(&mut *transaction)
    .await?;
    let id: Uuid = match created {
        Some(id) => {
            events::record_about(
                &mut transaction,
                About {
                    change: Some(id),
                    ..About::default()
                },
                events::CHANGE_CREATED,
                serde_json::json!({ "by": identity.login }),
            )
            .await?;
            id
        }
        None => {
            sqlx::query_scalar("SELECT id FROM changes WHERE status = 'draft' FOR UPDATE")
                .fetch_one(&mut *transaction)
                .await?
        }
    };
    let value = serde_json::to_value(&edit).map_err(|_| internal())?;
    sqlx::query("INSERT INTO change_edits (change_id, by, edit) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(&identity.login)
        .bind(&value)
        .execute(&mut *transaction)
        .await?;
    events::record_about(
        &mut transaction,
        About {
            host,
            change: Some(id),
            role,
        },
        events::CHANGE_EDITED,
        serde_json::json!({ "by": identity.login, "edit": value }),
    )
    .await?;
    let row: ChangeRow = sqlx::query_as(select_changes!("WHERE c.id = $1"))
        .bind(id)
        .fetch_one(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(change(row))))
}

fn internal() -> Failure {
    Failure::new(StatusCode::INTERNAL_SERVER_ERROR, "internal")
}

/// `POST /api/changes/{id}/discard`: closes a draft without applying it.
/// A change that is not a draft gets 409 `not_draft`.
pub async fn discard(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> ApiResult<Json<Change>> {
    let id = change_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    let current: Option<String> =
        sqlx::query_scalar("SELECT status FROM changes WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await?;
    match current.as_deref() {
        None => return Err(Failure::new(StatusCode::NOT_FOUND, "not_found")),
        Some("draft") => {}
        Some(_) => return Err(Failure::new(StatusCode::CONFLICT, "not_draft")),
    }
    sqlx::query("UPDATE changes SET status = 'discarded', closed_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    events::record_about(
        &mut transaction,
        About {
            change: Some(id),
            ..About::default()
        },
        events::CHANGE_DISCARDED,
        serde_json::json!({ "by": identity.login }),
    )
    .await?;
    let row: ChangeRow = sqlx::query_as(select_changes!("WHERE c.id = $1"))
        .bind(id)
        .fetch_one(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(Json(change(row)))
}

#[derive(Debug, Serialize)]
pub struct EditEntry {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub by: String,
    pub edit: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub config_hash: String,
    pub plan_hash: String,
    pub exit_code: i32,
    /// mise's JSON plan.
    pub plan: Option<serde_json::Value>,
    pub error: Option<String>,
    pub reported_at: DateTime<Utc>,
    /// The plan is of another config than the one the host gets now, so a
    /// newer plan is on its way.
    pub stale: bool,
}

#[derive(Debug, Serialize)]
pub struct AffectedHost {
    pub id: Uuid,
    pub name: String,
    pub status: Status,
    /// The config of the approved state.
    pub current_config: String,
    /// The config the change proposes, while it is a draft.
    pub proposed_config: Option<String>,
    pub plan: Option<Plan>,
}

#[derive(Debug, Serialize)]
pub struct ChangeDetail {
    #[serde(flatten)]
    pub change: Change,
    pub edits: Vec<EditEntry>,
    /// The hosts whose config the change alters, by name.
    pub hosts: Vec<AffectedHost>,
}

type PlanRow = (
    String,
    String,
    i32,
    Option<serde_json::Value>,
    Option<String>,
    DateTime<Utc>,
);

pub fn plan(row: PlanRow, config_hash: Option<&str>) -> Plan {
    let (hash, plan_hash, exit_code, plan, error, reported_at) = row;
    Plan {
        stale: config_hash.is_some_and(|expected| expected != hash),
        config_hash: hash,
        plan_hash,
        exit_code,
        plan,
        error,
        reported_at,
    }
}

type HostRow = (Uuid, String, Option<DateTime<Utc>>, Option<DateTime<Utc>>);

pub const SELECT_PLAN: &str =
    "SELECT config_hash, plan_hash, exit_code, plan, error, reported_at FROM host_plans
     WHERE host_id = $1 AND config = $2";

/// `GET /api/changes/{id}`: a change, its edits, and the plan of each host
/// it affects.
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ChangeDetail>> {
    let id = change_id(&id)?;
    let mut connection = state.pool.acquire().await?;
    let row: Option<ChangeRow> = sqlx::query_as(select_changes!("WHERE c.id = $1"))
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?;
    let change = change(row.ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?);
    let edits: Vec<(i64, DateTime<Utc>, String, serde_json::Value)> = sqlx::query_as(
        "SELECT id, at, by, edit FROM change_edits WHERE change_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(&mut *connection)
    .await?;
    let host_rows: Vec<HostRow> =
        sqlx::query_as("SELECT id, name, enrolled_at, last_seen_at FROM hosts ORDER BY name")
            .fetch_all(&mut *connection)
            .await?;
    let (approved, proposed) = desired::states(&mut connection).await?;
    let config = desired::change_config(id);
    let now = Utc::now();
    let mut hosts = Vec::new();
    for (host, name, enrolled_at, last_seen_at) in host_rows {
        let configs = desired::host_configs(
            host,
            &approved,
            proposed.as_ref().filter(|(draft, _)| *draft == id),
        );
        let proposed_config = configs.get(&config).cloned();
        let plan_row: Option<PlanRow> = sqlx::query_as(SELECT_PLAN)
            .bind(host)
            .bind(&config)
            .fetch_optional(&mut *connection)
            .await?;
        // A draft lists the hosts it alters now; a closed change lists the
        // hosts that planned it.
        if proposed_config.is_none() && (change.status == "draft" || plan_row.is_none()) {
            continue;
        }
        let expected = proposed_config.as_deref().map(desired::hash);
        hosts.push(AffectedHost {
            id: host,
            name,
            status: status(enrolled_at, last_seen_at, now),
            current_config: configs[desired::CURRENT].clone(),
            plan: plan_row.map(|row| plan(row, expected.as_deref())),
            proposed_config,
        });
    }
    Ok(Json(ChangeDetail {
        change,
        edits: edits
            .into_iter()
            .map(|(id, at, by, edit)| EditEntry { id, at, by, edit })
            .collect(),
        hosts,
    }))
}
