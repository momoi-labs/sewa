use std::collections::{BTreeSet, HashMap};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sewa_approval::{Approval, Assertion, HostPlan, Outcome, Progress, Rejection, Signed};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::{ApiResult, Failure};
use crate::events::{self, About};
use crate::roles::Named;
use crate::status::{self, Status};
use crate::whois::Identity;
use crate::{AppState, desired, passkeys};

const LOG_LIMIT: usize = 64 * 1024;

const LIST_LIMIT: i64 = 20;

fn conflict(code: &'static str) -> Failure {
    Failure::new(StatusCode::CONFLICT, code)
}

fn not_found() -> Failure {
    Failure::new(StatusCode::NOT_FOUND, "not_found")
}

fn internal() -> Failure {
    Failure::new(StatusCode::INTERNAL_SERVER_ERROR, "internal")
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub config_hash: String,
    pub plan_hash: String,
    pub exit_code: i32,
    pub plan: Option<serde_json::Value>,
    pub error: Option<String>,
    pub reported_at: DateTime<Utc>,
    pub received_at: Option<DateTime<Utc>>,
    pub stale: bool,
}

pub type PlanRow = (
    String,
    String,
    i32,
    Option<serde_json::Value>,
    Option<String>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

pub const SELECT_PLAN: &str =
    "SELECT config_hash, plan_hash, exit_code, plan, error, reported_at, received_at
     FROM host_plans
     WHERE host_id = $1 AND config = $2";

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
        received_at: received_at.and_then(|received| status::late(reported_at, received)),
    }
}

#[derive(Debug, Serialize)]
pub struct HostResult {
    pub plan_hash: Option<String>,
    pub log: Option<String>,
    pub reported_at: Option<DateTime<Utc>>,
    pub received_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StepEntry {
    pub step: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ApplyProgress {
    pub step: Option<String>,
    pub started_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub steps: Vec<StepEntry>,
}

#[derive(Debug, Serialize)]
pub struct SyncHost {
    pub id: Uuid,
    pub name: String,
    pub os: Option<String>,
    pub status: Status,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub roles: Vec<Named>,
    pub step: String,
    pub plan: Option<Plan>,
    pub result: Option<HostResult>,
    pub progress: Option<ApplyProgress>,
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
pub struct Sync {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub status: &'static str,
    pub hosts: Vec<SyncHost>,
    pub approvals: Vec<ApprovalEntry>,
}

fn ready(plan: Option<&Plan>) -> bool {
    plan.is_some_and(|plan| !plan.stale && matches!(plan.exit_code, 0 | 2))
}

fn sync_status(hosts: &[SyncHost]) -> &'static str {
    if hosts
        .iter()
        .any(|host| matches!(host.step.as_str(), "waiting" | "applying"))
    {
        return "applying";
    }
    let picked = hosts.iter().filter(|host| host.step == "picked");
    let mut status = "done";
    for host in picked {
        if ready(host.plan.as_ref()) {
            if host.plan.as_ref().is_some_and(|plan| plan.exit_code == 2) {
                status = "ready";
            }
        } else if host.status == Status::Connected
            && host.plan.as_ref().is_none_or(|plan| plan.stale)
        {
            return "planning";
        }
    }
    status
}

type SyncHostRow = (
    Uuid,
    String,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    String,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    sqlx::types::Json<Vec<StepEntry>>,
);

pub async fn load(connection: &mut PgConnection, id: Uuid) -> ApiResult<Sync> {
    let row: Option<(DateTime<Utc>, String)> =
        sqlx::query_as("SELECT created_at, created_by FROM syncs WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *connection)
            .await?;
    let (created_at, created_by) = row.ok_or_else(not_found)?;
    let rows: Vec<SyncHostRow> = sqlx::query_as(
        "SELECT h.id, h.name, h.os, h.disabled_at, h.enrolled_at, h.last_seen_at,
                s.status, s.plan_hash, s.log, s.reported_at, s.received_at,
                s.step, s.started_at, s.progress_at, s.steps
         FROM sync_hosts s JOIN hosts h ON h.id = s.host_id
         WHERE s.sync_id = $1 ORDER BY h.name",
    )
    .bind(id)
    .fetch_all(&mut *connection)
    .await?;
    let state = desired::State::load(&mut *connection).await?;
    let now = Utc::now();
    let mut hosts = Vec::new();
    for row in rows {
        let (
            host,
            name,
            os,
            disabled_at,
            enrolled_at,
            last_seen_at,
            step,
            plan_hash,
            log,
            reported,
            received,
            apply_step,
            started_at,
            progress_at,
            steps,
        ) = row;
        let expected = desired::hash(&desired::host_config(host, &state));
        let plan_row: Option<PlanRow> = sqlx::query_as(SELECT_PLAN)
            .bind(host)
            .bind(desired::CURRENT)
            .fetch_optional(&mut *connection)
            .await?;
        let signed = matches!(
            step.as_str(),
            "waiting" | "applying" | "applied" | "failed" | "replan"
        );
        hosts.push(SyncHost {
            id: host,
            name,
            os,
            status: status::status(disabled_at, enrolled_at, last_seen_at, now),
            last_seen_at,
            roles: state
                .roles_of(host)
                .into_iter()
                .map(|(id, name)| Named { id, name })
                .collect(),
            plan: plan_row.map(|row| plan(row, Some(&expected))),
            result: signed.then(|| HostResult {
                plan_hash,
                log,
                reported_at: reported,
                received_at: reported
                    .zip(received)
                    .and_then(|(reported, received)| status::late(reported, received)),
            }),
            progress: started_at.map(|started_at| ApplyProgress {
                step: apply_step,
                started_at,
                updated_at: progress_at,
                steps: steps.0,
            }),
            step,
        });
    }
    let approvals: Vec<(Uuid, String, String, DateTime<Utc>, i32)> = sqlx::query_as(
        "SELECT a.id, a.by, p.name, a.created_at, jsonb_array_length(a.hosts)
         FROM approvals a JOIN passkeys p ON p.id = a.passkey_id
         WHERE a.sync_id = $1 ORDER BY a.created_at, a.id",
    )
    .bind(id)
    .fetch_all(&mut *connection)
    .await?;
    Ok(Sync {
        id,
        created_at,
        created_by,
        status: sync_status(&hosts),
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
    })
}

fn sync_id(id: &str) -> ApiResult<Uuid> {
    id.parse().map_err(|_| not_found())
}

#[derive(Debug, Serialize)]
pub struct SyncEntry {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub status: &'static str,
    pub host_count: usize,
}

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<SyncEntry>>> {
    let mut connection = state.pool.acquire().await?;
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM syncs ORDER BY created_at DESC, id LIMIT $1")
            .bind(LIST_LIMIT)
            .fetch_all(&mut *connection)
            .await?;
    let mut entries = Vec::new();
    for id in ids {
        let sync = load(&mut connection, id).await?;
        entries.push(SyncEntry {
            id: sync.id,
            created_at: sync.created_at,
            created_by: sync.created_by,
            status: sync.status,
            host_count: sync.hosts.len(),
        });
    }
    Ok(Json(entries))
}

pub async fn get(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Sync>> {
    let id = sync_id(&id)?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(load(&mut connection, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct NewSync {
    pub hosts: Vec<Uuid>,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Json(new): Json<NewSync>,
) -> ApiResult<(StatusCode, Json<Sync>)> {
    let hosts: BTreeSet<Uuid> = new.hosts.into_iter().collect();
    if hosts.is_empty() {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "no_hosts"));
    }
    let hosts: Vec<Uuid> = hosts.into_iter().collect();
    let mut transaction = state.pool.begin().await?;
    sqlx::query("SELECT id FROM hosts WHERE id = ANY($1) ORDER BY id FOR UPDATE")
        .bind(&hosts)
        .fetch_all(&mut *transaction)
        .await?;
    let found: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hosts
         WHERE id = ANY($1) AND enrolled_at IS NOT NULL AND disabled_at IS NULL",
    )
    .bind(&hosts)
    .fetch_one(&mut *transaction)
    .await?;
    if found != hosts.len() as i64 {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "unknown_host"));
    }
    let id: Uuid = sqlx::query_scalar("INSERT INTO syncs (created_by) VALUES ($1) RETURNING id")
        .bind(&identity.login)
        .fetch_one(&mut *transaction)
        .await?;
    for host in &hosts {
        sqlx::query("INSERT INTO sync_hosts (sync_id, host_id) VALUES ($1, $2)")
            .bind(id)
            .bind(host)
            .execute(&mut *transaction)
            .await?;
        events::record_about(
            &mut transaction,
            About {
                host: Some(*host),
                sync: Some(id),
                role: None,
            },
            events::SYNC_CREATED,
            serde_json::json!({ "by": identity.login }),
        )
        .await?;
    }
    let sync = load(&mut transaction, id).await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(sync)))
}

struct Targets {
    approval: Approval,
    in_sync: Vec<Uuid>,
    skipped: Vec<Uuid>,
}

async fn targets(
    connection: &mut PgConnection,
    id: Uuid,
    approval_id: Uuid,
    only: Option<&[Uuid]>,
) -> ApiResult<Targets> {
    let sync = load(&mut *connection, id).await?;
    let mut targets = Targets {
        approval: Approval {
            approval_id,
            change_id: id,
            hosts: Vec::new(),
        },
        in_sync: Vec::new(),
        skipped: Vec::new(),
    };
    for host in &sync.hosts {
        if only.is_some_and(|only| !only.contains(&host.id))
            || host.status == Status::Disabled
            || !matches!(host.step.as_str(), "picked" | "failed" | "replan")
        {
            continue;
        }
        let instance: Option<Uuid> =
            sqlx::query_scalar("SELECT agent_instance_uid FROM hosts WHERE id = $1")
                .bind(host.id)
                .fetch_one(&mut *connection)
                .await?;
        match (&host.plan, instance) {
            (Some(plan), Some(instance_uid)) if ready(Some(plan)) && plan.exit_code == 2 => {
                targets.approval.hosts.push(HostPlan {
                    host_id: host.id,
                    instance_uid,
                    config_hash: plan.config_hash.clone(),
                    plan_hash: plan.plan_hash.clone(),
                });
            }
            (Some(plan), _) if host.step == "picked" && ready(Some(plan)) => {
                targets.in_sync.push(host.id);
            }
            _ if host.step == "picked" => targets.skipped.push(host.id),
            _ => {}
        }
    }
    targets.approval.hosts.sort_by_key(|host| host.host_id);
    if targets.approval.hosts.is_empty() && targets.in_sync.is_empty() {
        return Err(conflict("nothing_to_approve"));
    }
    Ok(targets)
}

#[derive(Debug, Default, Deserialize)]
pub struct Only {
    #[serde(default)]
    pub hosts: Option<Vec<Uuid>>,
}

#[derive(Debug, Serialize)]
pub struct ApprovalOptions {
    pub challenge: String,
    pub rp_id: String,
    pub allow_credentials: Vec<String>,
    pub signed_text: String,
    #[serde(flatten)]
    pub approval: Approval,
}

pub async fn options(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Json<ApprovalOptions>> {
    let id = sync_id(&id)?;
    let only: Only = if body.is_empty() {
        Only::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_body"))?
    };
    let mut connection = state.pool.acquire().await?;
    let targets = targets(&mut connection, id, Uuid::new_v4(), only.hosts.as_deref()).await?;
    let keys = passkeys::keys(&mut connection, &state.public_url, Some(&identity.login)).await?;
    if keys.keys.is_empty() {
        return Err(conflict("no_passkey"));
    }
    Ok(Json(ApprovalOptions {
        challenge: sewa_approval::encode(&targets.approval.challenge()),
        rp_id: keys.rp_id,
        allow_credentials: keys.keys.into_iter().map(|key| key.id).collect(),
        signed_text: targets.approval.signed_text(),
        approval: targets.approval,
    }))
}

#[derive(Debug, Deserialize)]
pub struct Approve {
    pub approval_id: Uuid,
    #[serde(flatten)]
    pub assertion: Assertion,
    #[serde(default)]
    pub hosts: Option<Vec<Uuid>>,
}

pub async fn approve(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    Json(approve): Json<Approve>,
) -> ApiResult<Json<Sync>> {
    let id = sync_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    sqlx::query(
        "SELECT id FROM hosts
         WHERE id IN (SELECT host_id FROM sync_hosts WHERE sync_id = $1)
         ORDER BY id FOR UPDATE",
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await?;
    let found: Option<Uuid> = sqlx::query_scalar("SELECT id FROM syncs WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?;
    found.ok_or_else(not_found)?;
    let targets = targets(
        &mut transaction,
        id,
        approve.approval_id,
        approve.hosts.as_deref(),
    )
    .await?;
    let keys = passkeys::keys(&mut transaction, &state.public_url, Some(&identity.login)).await?;
    let approver = sewa_approval::verify(&keys, &approve.assertion, &targets.approval.challenge())
        .map_err(|rejection| match rejection {
            Rejection::WrongChallenge => conflict("plans_changed"),
            _ => Failure::new(StatusCode::BAD_REQUEST, "invalid_assertion"),
        })?;
    for (hosts, step) in [(&targets.in_sync, "in_sync"), (&targets.skipped, "skipped")] {
        sqlx::query(
            "UPDATE sync_hosts SET status = $3
             WHERE sync_id = $1 AND host_id = ANY($2) AND status = 'picked'",
        )
        .bind(id)
        .bind(hosts)
        .bind(step)
        .execute(&mut *transaction)
        .await?;
    }
    if !targets.approval.hosts.is_empty() {
        let passkey: Uuid = sqlx::query_scalar("SELECT id FROM passkeys WHERE credential_id = $1")
            .bind(sewa_approval::decode(&approver.id).unwrap_or_default())
            .fetch_one(&mut *transaction)
            .await?;
        let hosts = serde_json::to_value(&targets.approval.hosts).map_err(|_| internal())?;
        let assertion = serde_json::to_value(&approve.assertion).map_err(|_| internal())?;
        let approval_id = targets.approval.approval_id;
        let inserted = sqlx::query(
            "INSERT INTO approvals (id, sync_id, by, passkey_id, hosts, assertion)
             VALUES ($6, $1, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING",
        )
        .bind(id)
        .bind(&identity.login)
        .bind(passkey)
        .bind(&hosts)
        .bind(&assertion)
        .bind(approval_id)
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() == 0 {
            return Err(conflict("plans_changed"));
        }
        for host in &targets.approval.hosts {
            sqlx::query(
                "UPDATE sync_hosts SET status = 'waiting', approval_id = $3, plan_hash = $4,
                     log = NULL, reported_at = NULL, received_at = NULL,
                     step = NULL, started_at = NULL, progress_at = NULL, steps = '[]'
                 WHERE sync_id = $1 AND host_id = $2",
            )
            .bind(id)
            .bind(host.host_id)
            .bind(approval_id)
            .bind(&host.plan_hash)
            .execute(&mut *transaction)
            .await?;
            events::record_about(
                &mut transaction,
                About {
                    host: Some(host.host_id),
                    sync: Some(id),
                    role: None,
                },
                events::SYNC_APPROVED,
                serde_json::json!({
                    "by": identity.login,
                    "approval_id": approval_id,
                    "plan_hash": host.plan_hash,
                }),
            )
            .await?;
        }
    }
    let sync = load(&mut transaction, id).await?;
    transaction.commit().await?;
    Ok(Json(sync))
}

pub async fn pending(pool: &sqlx::PgPool, host: Uuid) -> sqlx::Result<Option<Signed>> {
    let row: Option<(Uuid, Uuid, serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT a.id, a.sync_id, a.hosts, a.assertion
         FROM sync_hosts s JOIN approvals a ON a.id = s.approval_id
         WHERE s.host_id = $1 AND s.status = 'waiting'
         ORDER BY a.created_at, a.id
         LIMIT 1",
    )
    .bind(host)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(approval_id, sync_id, hosts, assertion)| {
        Some(Signed {
            approval: Approval {
                approval_id,
                change_id: sync_id,
                hosts: serde_json::from_value(hosts).ok()?,
            },
            assertion: serde_json::from_value(assertion).ok()?,
        })
    }))
}

pub async fn save_outcome(pool: &sqlx::PgPool, host: Uuid, outcome: &Outcome) -> sqlx::Result<()> {
    let kind = match outcome.status.as_str() {
        "applied" => events::PLAN_APPLIED,
        "failed" => events::APPLY_FAILED,
        "replan" => events::APPLY_REPLAN,
        _ => {
            tracing::warn!(host = %host, status = outcome.status, "an unknown apply status");
            return Ok(());
        }
    };
    let log = tail(&outcome.log);
    let received = Utc::now();
    let reported = status::reported_at(outcome.at, received);
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT 1 FROM hosts WHERE id = $1 FOR UPDATE")
        .bind(host)
        .fetch_optional(&mut *transaction)
        .await?;
    let updated = sqlx::query(
        "UPDATE sync_hosts SET status = $4, log = $5, reported_at = $6, received_at = $7
         WHERE sync_id = $1 AND host_id = $2 AND approval_id = $3
             AND status IN ('waiting', 'applying')",
    )
    .bind(outcome.change_id)
    .bind(host)
    .bind(outcome.approval_id)
    .bind(&outcome.status)
    .bind(&log)
    .bind(reported)
    .bind(received)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() == 1 {
        let mut data = serde_json::json!({
            "approval_id": outcome.approval_id,
            "plan_hash": outcome.plan_hash,
        });
        if let Some(late) = status::late(reported, received) {
            data["received_at"] = serde_json::json!(late);
        }
        events::record_at(
            &mut transaction,
            About {
                host: Some(host),
                sync: Some(outcome.change_id),
                role: None,
            },
            kind,
            data,
            reported,
        )
        .await?;
    }
    transaction.commit().await
}

fn tail(log: &str) -> String {
    let mut log = log.replace('\0', "");
    if log.len() > LOG_LIMIT {
        let mut start = log.len() - LOG_LIMIT;
        while !log.is_char_boundary(start) {
            start += 1;
        }
        log = format!("…{}", &log[start..]);
    }
    log
}

pub async fn save_progress(
    pool: &sqlx::PgPool,
    host: Uuid,
    progress: &Progress,
) -> sqlx::Result<()> {
    if !matches!(progress.step.as_str(), "native" | "mise") {
        tracing::warn!(host = %host, step = progress.step, "an unknown apply step");
        return Ok(());
    }
    let received = Utc::now();
    let at = status::reported_at(Some(progress.at), received);
    let started = status::reported_at(Some(progress.started_at), received);
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT 1 FROM hosts WHERE id = $1 FOR UPDATE")
        .bind(host)
        .fetch_optional(&mut *transaction)
        .await?;
    sqlx::query(
        "UPDATE sync_hosts SET status = 'applying', step = $4, log = $5,
             started_at = coalesce(started_at, $6), progress_at = $7,
             steps = CASE WHEN step IS DISTINCT FROM $4
                 THEN steps || jsonb_build_array(jsonb_build_object('step', $4::text, 'at', $7::timestamptz))
                 ELSE steps END
         WHERE sync_id = $1 AND host_id = $2 AND approval_id = $3
             AND status IN ('waiting', 'applying')
             AND (progress_at IS NULL OR progress_at <= $7)",
    )
    .bind(progress.change_id)
    .bind(host)
    .bind(progress.approval_id)
    .bind(&progress.step)
    .bind(tail(&progress.log))
    .bind(started)
    .bind(at)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await
}

#[derive(Debug, Serialize)]
pub struct EditEntry {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub kind: String,
    pub role: Option<Named>,
    pub host: Option<Named>,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct Edits {
    pub since: Option<DateTime<Utc>>,
    pub edits: Vec<EditEntry>,
}

const EDIT_LIMIT: i64 = 200;

pub async fn edits(State(state): State<AppState>) -> ApiResult<Json<Edits>> {
    let mut connection = state.pool.acquire().await?;
    let since: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT max(created_at) FROM syncs")
        .fetch_one(&mut *connection)
        .await?;
    type Row = (
        i64,
        DateTime<Utc>,
        String,
        Option<Uuid>,
        Option<String>,
        Option<Uuid>,
        Option<String>,
        serde_json::Value,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT e.id, e.at, e.kind, r.id, r.name, h.id, h.name, e.data
         FROM events e
         LEFT JOIN roles r ON r.id = e.role_id
         LEFT JOIN hosts h ON h.id = e.host_id
         WHERE e.kind = ANY($1) AND ($2::timestamptz IS NULL OR e.at > $2)
         ORDER BY e.at, e.id
         LIMIT $3",
    )
    .bind([
        events::ROLE_SAVED,
        events::HOST_ROLES_SET,
        events::HOST_ITEMS_SET,
        events::DRIFT_ADOPTED,
    ])
    .bind(since)
    .bind(EDIT_LIMIT)
    .fetch_all(&mut *connection)
    .await?;
    let named = |id: Option<Uuid>, name: Option<String>| {
        Some(Named {
            id: id?,
            name: name?,
        })
    };
    Ok(Json(Edits {
        since,
        edits: rows
            .into_iter()
            .map(
                |(id, at, kind, role, role_name, host, host_name, data)| EditEntry {
                    id,
                    at,
                    kind,
                    role: named(role, role_name),
                    host: named(host, host_name),
                    data,
                },
            )
            .collect(),
    }))
}

pub async fn open_steps(
    connection: &mut PgConnection,
    host: Uuid,
) -> sqlx::Result<HashMap<Uuid, String>> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT sync_id, status FROM sync_hosts WHERE host_id = $1 AND status IN ('picked', 'waiting', 'applying')",
    )
    .bind(host)
    .fetch_all(connection)
    .await?;
    Ok(rows.into_iter().collect())
}
