//! Syncs, under `/api/syncs`: an admin picks hosts, reads each host's plan
//! of its desired state, signs the plans with a passkey, and each agent
//! applies its plan and reports back.
//!
//! A host's step in a sync:
//!
//! - `picked`: its plan is live, read from the agent's last plan of
//!   `current`, until a signature;
//! - `in_sync` or `skipped` at the signature: nothing to do, or no plan to
//!   sign (offline, not planned yet, or an error);
//! - `waiting` once signed, until its agent reports `applied`, `failed`,
//!   or `replan` (its plan changed before it applied). A failed or replan
//!   host can be signed again.
//!
//! An approval binds the sync id and, for each host, the agent instance,
//! the config hash, and the plan hash the person read (see the
//! `sewa-approval` crate). The approval keeps its `change_id` field name,
//! which agents already installed read, and the field carries the sync id.

use std::collections::{BTreeSet, HashMap};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sewa_approval::{Approval, Assertion, HostPlan, Outcome, Rejection, Signed};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::{ApiResult, Failure};
use crate::events::{self, About};
use crate::roles::Named;
use crate::status::{self, Status};
use crate::whois::Identity;
use crate::{AppState, desired, passkeys};

/// The longest log the controller keeps of one apply.
const LOG_LIMIT: usize = 64 * 1024;

/// How many syncs `GET /api/syncs` returns.
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
    /// The agent's JSON plan: mise's, with the packages the agent installs
    /// itself first.
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
    /// The agent's apply log.
    pub log: Option<String>,
    pub reported_at: Option<DateTime<Utc>>,
    pub received_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct SyncHost {
    pub id: Uuid,
    pub name: String,
    pub os: Option<String>,
    pub status: Status,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub roles: Vec<Named>,
    /// `picked`, `in_sync`, `skipped`, `waiting`, `applied`, `failed`, or
    /// `replan`.
    pub step: String,
    /// The host's last plan of its desired state.
    pub plan: Option<Plan>,
    /// What the agent reported, once signed.
    pub result: Option<HostResult>,
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
    /// `planning` (a picked host still plans), `ready` (plans to sign),
    /// `applying` (a signed host has not reported), or `done`.
    pub status: &'static str,
    pub hosts: Vec<SyncHost>,
    pub approvals: Vec<ApprovalEntry>,
}

/// Whether a plan can be signed: an agent instance, a plan of the config
/// the host gets now, and a plan mise could make.
fn ready(plan: Option<&Plan>) -> bool {
    plan.is_some_and(|plan| !plan.stale && matches!(plan.exit_code, 0 | 2))
}

fn sync_status(hosts: &[SyncHost]) -> &'static str {
    if hosts.iter().any(|host| host.step == "waiting") {
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
    String,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

/// A sync as the API shows it, read in the caller's connection.
pub async fn load(connection: &mut PgConnection, id: Uuid) -> ApiResult<Sync> {
    let row: Option<(DateTime<Utc>, String)> =
        sqlx::query_as("SELECT created_at, created_by FROM syncs WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *connection)
            .await?;
    let (created_at, created_by) = row.ok_or_else(not_found)?;
    let rows: Vec<SyncHostRow> = sqlx::query_as(
        "SELECT h.id, h.name, h.os, h.enrolled_at, h.last_seen_at,
                s.status, s.plan_hash, s.log, s.reported_at, s.received_at
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
        let (host, name, os, enrolled_at, last_seen_at, step, plan_hash, log, reported, received) =
            row;
        let expected = desired::hash(&desired::host_config(host, &state));
        let plan_row: Option<PlanRow> = sqlx::query_as(SELECT_PLAN)
            .bind(host)
            .bind(desired::CURRENT)
            .fetch_optional(&mut *connection)
            .await?;
        let signed = matches!(step.as_str(), "waiting" | "applied" | "failed" | "replan");
        hosts.push(SyncHost {
            id: host,
            name,
            os,
            status: status::status(enrolled_at, last_seen_at, now),
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

/// `GET /api/syncs`: the latest syncs, newest first.
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

/// `GET /api/syncs/{id}`: a sync and where each of its hosts stands.
pub async fn get(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Sync>> {
    let id = sync_id(&id)?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(load(&mut connection, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct NewSync {
    pub hosts: Vec<Uuid>,
}

/// `POST /api/syncs`: starts a sync of the hosts picked, which must be
/// enrolled. 400 `no_hosts` or `unknown_host`. Records `sync.created` for
/// each host.
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
    let found: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hosts WHERE id = ANY($1) AND enrolled_at IS NOT NULL",
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

/// What a signature of the sync would cover now: the hosts to sign, the
/// picked hosts already in sync, and the picked hosts without a plan to
/// sign. Only the hosts in `only` count, when given. 404 `not_found`, 409
/// `nothing_to_approve`.
struct Targets {
    approval: Approval,
    in_sync: Vec<Uuid>,
    skipped: Vec<Uuid>,
}

async fn targets(
    connection: &mut PgConnection,
    id: Uuid,
    only: Option<&[Uuid]>,
) -> ApiResult<Targets> {
    let sync = load(&mut *connection, id).await?;
    let mut targets = Targets {
        approval: Approval {
            change_id: id,
            hosts: Vec::new(),
        },
        in_sync: Vec::new(),
        skipped: Vec::new(),
    };
    for host in &sync.hosts {
        if only.is_some_and(|only| !only.contains(&host.id))
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
    /// Sign only these hosts, such as one failed host to retry.
    #[serde(default)]
    pub hosts: Option<Vec<Uuid>>,
}

#[derive(Debug, Serialize)]
pub struct ApprovalOptions {
    /// Unpadded base64url: the SHA-256 of `signed_text`.
    pub challenge: String,
    pub rp_id: String,
    /// The signed-in user's credential ids.
    pub allow_credentials: Vec<String>,
    /// The text the challenge hashes.
    pub signed_text: String,
    #[serde(flatten)]
    pub approval: Approval,
}

/// `POST /api/syncs/{id}/approval`: what a signature would sign now, and
/// the challenge to sign. 409 `no_passkey` when the signed-in user has no
/// passkey, `nothing_to_approve` when no host has a plan to sign or is in
/// sync. With no host to sign, the challenge signs an empty list, and the
/// approve call only marks the hosts in sync.
pub async fn options(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Json<ApprovalOptions>> {
    let id = sync_id(&id)?;
    // The body is optional.
    let only: Only = if body.is_empty() {
        Only::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_body"))?
    };
    let mut connection = state.pool.acquire().await?;
    let targets = targets(&mut connection, id, only.hosts.as_deref()).await?;
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
    #[serde(flatten)]
    pub assertion: Assertion,
    #[serde(default)]
    pub hosts: Option<Vec<Uuid>>,
}

/// `POST /api/syncs/{id}/approve`: signs the sync with an assertion of the
/// challenge from `options`, with the same `hosts`. The assertion must
/// come from a passkey of the signed-in user (400 `invalid_assertion`
/// otherwise), and sign the plans as they are now (409 `plans_changed`
/// otherwise). Each signed host waits for its agent; a picked host in sync
/// becomes `in_sync`, and one without a plan `skipped`.
pub async fn approve(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    Json(approve): Json<Approve>,
) -> ApiResult<Json<Sync>> {
    let id = sync_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    let found: Option<Uuid> = sqlx::query_scalar("SELECT id FROM syncs WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?;
    found.ok_or_else(not_found)?;
    let targets = targets(&mut transaction, id, approve.hosts.as_deref()).await?;
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
        let approval_id: Uuid = sqlx::query_scalar(
            "INSERT INTO approvals (sync_id, by, passkey_id, hosts, assertion)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(id)
        .bind(&identity.login)
        .bind(passkey)
        .bind(&hosts)
        .bind(&assertion)
        .fetch_one(&mut *transaction)
        .await?;
        for host in &targets.approval.hosts {
            sqlx::query(
                "UPDATE sync_hosts SET status = 'waiting', approval_id = $3, plan_hash = $4,
                     log = NULL, reported_at = NULL, received_at = NULL
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

/// The oldest approval a host has not reported on, as the agent gets it.
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
            approval_id,
            approval: Approval {
                change_id: sync_id,
                hosts: serde_json::from_value(hosts).ok()?,
            },
            assertion: serde_json::from_value(assertion).ok()?,
        })
    }))
}

/// Keeps what an agent did with an approval it was sent, once, and
/// records `plan.applied`, `apply.failed`, or `apply.replan`.
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
    let mut log = outcome.log.replace('\0', "");
    if log.len() > LOG_LIMIT {
        let mut start = log.len() - LOG_LIMIT;
        while !log.is_char_boundary(start) {
            start += 1;
        }
        log = format!("…{}", &log[start..]);
    }
    let received = Utc::now();
    let reported = status::reported_at(outcome.at, received);
    let mut transaction = pool.begin().await?;
    let updated = sqlx::query(
        "UPDATE sync_hosts SET status = $4, log = $5, reported_at = $6, received_at = $7
         WHERE sync_id = $1 AND host_id = $2 AND approval_id = $3 AND status = 'waiting'",
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
    /// When the last sync started, or null before the first.
    pub since: Option<DateTime<Utc>>,
    /// The edits of roles and hosts since then, oldest first.
    pub edits: Vec<EditEntry>,
}

/// How many edits `GET /api/syncs/edits` returns.
const EDIT_LIMIT: i64 = 200;

/// `GET /api/syncs/edits`: the edits since the last sync started.
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

/// The steps of each host in the open syncs, for a host page: whether a
/// sync waits on it.
pub async fn open_steps(
    connection: &mut PgConnection,
    host: Uuid,
) -> sqlx::Result<HashMap<Uuid, String>> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT sync_id, status FROM sync_hosts WHERE host_id = $1 AND status IN ('picked', 'waiting')",
    )
    .bind(host)
    .fetch_all(connection)
    .await?;
    Ok(rows.into_iter().collect())
}
