//! `/api/changes`: the draft change and its edits, and each host's plan of
//! it. A change stays a draft until it is approved (see `approvals`) or
//! discarded. Every edit is a `change.edited` event. A revert change has no
//! edits: it applies the approved state again on one host.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
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
    /// The host a revert change applies the approved state on again, or
    /// null for a change of edits.
    pub revert_host: Option<Named>,
}

#[derive(Debug, Serialize)]
pub struct Named {
    pub id: Uuid,
    pub name: String,
}

type ChangeRow = (
    Uuid,
    String,
    DateTime<Utc>,
    String,
    Option<DateTime<Utc>>,
    i64,
    Option<Uuid>,
    Option<String>,
);

/// A query of [`ChangeRow`]s: `SELECT <its columns> FROM changes c` and the
/// rest.
macro_rules! select_changes {
    ($rest:literal) => {
        concat!(
            "SELECT c.id, c.status, c.created_at, c.created_by, c.closed_at,
                    (SELECT count(*) FROM change_edits e WHERE e.change_id = c.id),
                    c.revert_host, (SELECT name FROM hosts h WHERE h.id = c.revert_host)
             FROM changes c ",
            $rest
        )
    };
}

fn change(row: ChangeRow) -> Change {
    let (id, status, created_at, created_by, closed_at, edit_count, revert_host, revert_name) = row;
    Change {
        id,
        status,
        created_at,
        created_by,
        closed_at,
        edit_count,
        revert_host: revert_host.map(|id| Named {
            id,
            name: revert_name.unwrap_or_default(),
        }),
    }
}

/// One change, in the caller's transaction.
pub async fn load(connection: &mut PgConnection, id: Uuid) -> sqlx::Result<Change> {
    let row: ChangeRow = sqlx::query_as(select_changes!("WHERE c.id = $1"))
        .bind(id)
        .fetch_one(connection)
        .await?;
    Ok(change(row))
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
    let mut transaction = state.pool.begin().await?;
    let id = add_edit_in(&mut transaction, &identity.login, &edit).await?;
    let change = load(&mut transaction, id).await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(change)))
}

/// Adds one edit to the draft in the caller's transaction, and creates the
/// draft when there is none. Returns the draft's id.
pub async fn add_edit_in(
    transaction: &mut PgConnection,
    login: &str,
    edit: &Edit,
) -> ApiResult<Uuid> {
    if !edit.is_valid() {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_item"));
    }
    let (role, host) = edit.targets();
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
         ON CONFLICT ((true)) WHERE status = 'draft' AND revert_host IS NULL DO NOTHING
         RETURNING id",
    )
    .bind(login)
    .fetch_optional(&mut *transaction)
    .await?;
    let id: Uuid =
        match created {
            Some(id) => {
                events::record_about(
                    &mut *transaction,
                    About {
                        change: Some(id),
                        ..About::default()
                    },
                    events::CHANGE_CREATED,
                    serde_json::json!({ "by": login }),
                )
                .await?;
                id
            }
            None => sqlx::query_scalar(
                "SELECT id FROM changes WHERE status = 'draft' AND revert_host IS NULL FOR UPDATE",
            )
            .fetch_one(&mut *transaction)
            .await?,
        };
    let value = serde_json::to_value(edit).map_err(|_| internal())?;
    sqlx::query("INSERT INTO change_edits (change_id, by, edit) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(login)
        .bind(&value)
        .execute(&mut *transaction)
        .await?;
    events::record_about(
        &mut *transaction,
        About {
            host,
            change: Some(id),
            role,
        },
        events::CHANGE_EDITED,
        serde_json::json!({ "by": login, "edit": value }),
    )
    .await?;
    Ok(id)
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
    /// When the agent planned, by its clock.
    pub reported_at: DateTime<Utc>,
    /// When the controller got the plan, if late: the agent kept it while
    /// it could not reach the controller.
    pub received_at: Option<DateTime<Utc>>,
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
    /// The config the change proposes, while it is a draft. A revert
    /// proposes the current config.
    pub proposed_config: Option<String>,
    pub plan: Option<Plan>,
    /// What the host did with the change's approval, once approved.
    pub result: Option<HostResult>,
}

#[derive(Debug, Serialize)]
pub struct HostResult {
    /// `waiting`, `applied`, `failed`, or `replan`: the plan changed before
    /// the host applied, and the change needs a new approval for it.
    pub status: String,
    /// The plan hash the approval signed.
    pub plan_hash: String,
    /// mise's output, or why the agent refused.
    pub log: Option<String>,
    /// When the agent finished, by its clock.
    pub reported_at: Option<DateTime<Utc>>,
    /// When the controller got the result, if late, as with a plan.
    pub received_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct ApprovalEntry {
    pub id: Uuid,
    pub by: String,
    pub passkey: String,
    pub created_at: DateTime<Utc>,
    pub host_count: i32,
}

#[derive(Debug, Serialize)]
pub struct ChangeDetail {
    #[serde(flatten)]
    pub change: Change,
    pub edits: Vec<EditEntry>,
    /// The hosts whose config the change alters, by name.
    pub hosts: Vec<AffectedHost>,
    /// The approvals of the change, oldest first.
    pub approvals: Vec<ApprovalEntry>,
}

type PlanRow = (
    String,
    String,
    i32,
    Option<serde_json::Value>,
    Option<String>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

pub fn plan(row: PlanRow, config_hash: Option<&str>) -> Plan {
    let (hash, plan_hash, exit_code, plan, error, reported_at, received_at) = row;
    Plan {
        stale: config_hash.is_some_and(|expected| expected != hash),
        config_hash: hash,
        plan_hash,
        exit_code,
        plan,
        error,
        reported_at,
        received_at: received_at.and_then(|received| crate::status::late(reported_at, received)),
    }
}

type HostRow = (Uuid, String, Option<DateTime<Utc>>, Option<DateTime<Utc>>);

type ResultRow = (
    String,
    String,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);
type HostResultRow = (
    Uuid,
    String,
    String,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

pub const SELECT_PLAN: &str =
    "SELECT config_hash, plan_hash, exit_code, plan, error, reported_at, received_at
     FROM host_plans
     WHERE host_id = $1 AND config = $2";

/// `GET /api/changes/{id}`: a change, its edits, the plan of each host it
/// affects, and what each host did once it was approved.
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
    let results: Vec<HostResultRow> = sqlx::query_as(
        "SELECT host_id, status, plan_hash, log, reported_at, received_at FROM change_hosts
             WHERE change_id = $1",
    )
    .bind(id)
    .fetch_all(&mut *connection)
    .await?;
    let mut results: HashMap<Uuid, ResultRow> = results
        .into_iter()
        .map(|(host, status, plan_hash, log, at, received)| {
            (host, (status, plan_hash, log, at, received))
        })
        .collect();
    let approvals: Vec<(Uuid, String, String, DateTime<Utc>, i32)> = sqlx::query_as(
        "SELECT a.id, a.by, p.name, a.created_at, jsonb_array_length(a.hosts)
         FROM approvals a JOIN passkeys p ON p.id = a.passkey_id
         WHERE a.change_id = $1 ORDER BY a.created_at, a.id",
    )
    .bind(id)
    .fetch_all(&mut *connection)
    .await?;
    let (approved, proposed) = desired::states(&mut connection).await?;
    let config = desired::change_config(id);
    let revert = change.revert_host.as_ref().map(|host| host.id);
    let draft = change.status == "draft";
    let now = Utc::now();
    let mut hosts = Vec::new();
    for (host, name, enrolled_at, last_seen_at) in host_rows {
        let configs = desired::host_configs(
            host,
            &approved,
            proposed.as_ref().filter(|(draft, _)| *draft == id),
        );
        let current_config = configs[desired::CURRENT].clone();
        let result = results.remove(&host);
        if revert.is_some_and(|revert| revert != host) {
            continue;
        }
        // The plan to show: of `current` for a revert and for a host that
        // needs a new approval, else of the change's config.
        let replan = result
            .as_ref()
            .is_some_and(|(status, ..)| status == "replan");
        let (plan_config, proposed_config) = if revert.is_some() || replan {
            (
                desired::CURRENT.to_owned(),
                draft.then(|| current_config.clone()),
            )
        } else {
            (config.clone(), configs.get(&config).cloned())
        };
        let plan_row: Option<PlanRow> = sqlx::query_as(SELECT_PLAN)
            .bind(host)
            .bind(&plan_config)
            .fetch_optional(&mut *connection)
            .await?;
        // A draft lists the hosts it alters now; a closed change lists the
        // hosts it was approved for, or that planned it.
        if revert.is_none()
            && proposed_config.is_none()
            && result.is_none()
            && (draft || plan_row.is_none())
        {
            continue;
        }
        let expected = if plan_config == desired::CURRENT {
            Some(desired::hash(&current_config))
        } else {
            proposed_config.as_deref().map(desired::hash)
        };
        hosts.push(AffectedHost {
            id: host,
            name,
            status: status(enrolled_at, last_seen_at, now),
            current_config,
            plan: plan_row.map(|row| plan(row, expected.as_deref())),
            proposed_config,
            result: result.map(
                |(status, plan_hash, log, reported_at, received_at)| HostResult {
                    status,
                    plan_hash,
                    log,
                    reported_at,
                    received_at: reported_at
                        .zip(received_at)
                        .and_then(|(reported, received)| crate::status::late(reported, received)),
                },
            ),
        });
    }
    Ok(Json(ChangeDetail {
        change,
        edits: edits
            .into_iter()
            .map(|(id, at, by, edit)| EditEntry { id, at, by, edit })
            .collect(),
        hosts,
        approvals: approvals
            .into_iter()
            .map(|(id, by, passkey, created_at, host_count)| ApprovalEntry {
                id,
                by,
                passkey,
                created_at,
                host_count,
            })
            .collect(),
    }))
}
