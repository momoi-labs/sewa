//! Approving a change with a passkey, sending the approval to each agent,
//! and keeping what each agent did with it.
//!
//! An approval binds the change id and, for each host, the agent instance,
//! the config hash, and the plan hash the person read (see the
//! `sewa-approval` crate). What a change's approval covers:
//!
//! - a draft of edits: each host whose config the draft changes, with its
//!   plan of `change/<id>`. Approving writes the edits into the approved
//!   state, so it becomes each host's new `current`;
//! - a revert: its one host, with its plan of `current`;
//! - an approved change: the hosts that need a new approval, because their
//!   plan changed before they applied, with their plan of `current`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Serialize;
use sewa_approval::{Approval, Assertion, HostPlan, Outcome, Rejection, Signed};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::AppState;
use crate::changes::{self, Change};
use crate::desired;
use crate::error::{ApiResult, Failure};
use crate::events::{self, About};
use crate::passkeys;
use crate::whois::Identity;

/// The longest log the controller keeps of one apply.
const LOG_LIMIT: usize = 64 * 1024;

fn conflict(code: &'static str) -> Failure {
    Failure::new(StatusCode::CONFLICT, code)
}

/// What a change's next approval covers, read in the caller's transaction.
/// Fails with 404 `not_found`, or 409 `not_approvable` (discarded),
/// `nothing_to_approve`, or `plans_not_ready` (a host has no plan of the
/// config it gets now, or an error plan, or no agent).
async fn targets(connection: &mut PgConnection, change: Uuid) -> ApiResult<Approval> {
    let row: Option<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT status, revert_host FROM changes WHERE id = $1")
            .bind(change)
            .fetch_optional(&mut *connection)
            .await?;
    let (status, revert_host) = row.ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    let (approved, proposed) = desired::states(&mut *connection).await?;
    let current = |host: Uuid| desired::render(&approved.host_items(host));
    // Each host, the config it plans, and that config's body.
    let mut wanted: Vec<(Uuid, String, String)> = Vec::new();
    match (status.as_str(), revert_host) {
        ("draft", None) => {
            let proposed = proposed
                .filter(|(draft, _)| *draft == change)
                .ok_or(conflict("not_approvable"))?;
            let config = desired::change_config(change);
            let hosts: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM hosts ORDER BY id")
                .fetch_all(&mut *connection)
                .await?;
            for host in hosts {
                let configs = desired::host_configs(host, &approved, Some(&proposed));
                if let Some(body) = configs.get(&config) {
                    wanted.push((host, config.clone(), body.clone()));
                }
            }
        }
        ("draft", Some(host)) => {
            wanted.push((host, desired::CURRENT.to_owned(), current(host)));
        }
        ("approved", _) => {
            let hosts: Vec<Uuid> = sqlx::query_scalar(
                "SELECT host_id FROM change_hosts WHERE change_id = $1 AND status = 'replan'
                 ORDER BY host_id",
            )
            .bind(change)
            .fetch_all(&mut *connection)
            .await?;
            for host in hosts {
                wanted.push((host, desired::CURRENT.to_owned(), current(host)));
            }
        }
        _ => return Err(conflict("not_approvable")),
    }
    if wanted.is_empty() {
        return Err(conflict("nothing_to_approve"));
    }
    let mut hosts = Vec::new();
    for (host, config, body) in wanted {
        let row: Option<PlanRow> = sqlx::query_as(
            "SELECT h.agent_instance_uid, p.config_hash, p.plan_hash, p.exit_code
                 FROM hosts h
                 LEFT JOIN host_plans p ON p.host_id = h.id AND p.config = $2
                 WHERE h.id = $1",
        )
        .bind(host)
        .bind(&config)
        .fetch_optional(&mut *connection)
        .await?;
        let config_hash = desired::hash(&body);
        match row {
            Some((Some(instance_uid), Some(planned), Some(plan_hash), Some(0 | 2)))
                if planned == config_hash =>
            {
                hosts.push(HostPlan {
                    host_id: host,
                    instance_uid,
                    config_hash,
                    plan_hash,
                });
            }
            _ => return Err(conflict("plans_not_ready")),
        }
    }
    Ok(Approval {
        change_id: change,
        hosts,
    })
}

/// A host's agent instance and its plan of a config, if any.
type PlanRow = (Option<Uuid>, Option<String>, Option<String>, Option<i32>);

fn change_id(id: &str) -> ApiResult<Uuid> {
    id.parse()
        .map_err(|_| Failure::new(StatusCode::NOT_FOUND, "not_found"))
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

/// `POST /api/changes/{id}/approval`: what an approval of the change would
/// sign now, and the challenge to sign. 409 `no_passkey` when the signed-in
/// user has no passkey.
pub async fn options(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> ApiResult<Json<ApprovalOptions>> {
    let id = change_id(&id)?;
    let mut connection = state.pool.acquire().await?;
    let approval = targets(&mut connection, id).await?;
    let keys = passkeys::keys(&mut connection, &state.public_url, Some(&identity.login)).await?;
    if keys.keys.is_empty() {
        return Err(conflict("no_passkey"));
    }
    Ok(Json(ApprovalOptions {
        challenge: sewa_approval::encode(&approval.challenge()),
        rp_id: keys.rp_id,
        allow_credentials: keys.keys.into_iter().map(|key| key.id).collect(),
        signed_text: approval.signed_text(),
        approval,
    }))
}

/// `POST /api/changes/{id}/approve`: approves the change with an assertion
/// of the challenge from `options`. The assertion must come from a passkey
/// of the signed-in user (400 `invalid_assertion` otherwise), and sign the
/// plans as they are now (409 `plans_changed` otherwise).
pub async fn approve(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    Json(assertion): Json<Assertion>,
) -> ApiResult<Json<Change>> {
    let id = change_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM changes WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await?;
    let status = status.ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    let approval = targets(&mut transaction, id).await?;
    let keys = passkeys::keys(&mut transaction, &state.public_url, Some(&identity.login)).await?;
    let approver =
        sewa_approval::verify(&keys, &assertion, &approval.challenge()).map_err(|rejection| {
            match rejection {
                Rejection::WrongChallenge => conflict("plans_changed"),
                _ => Failure::new(StatusCode::BAD_REQUEST, "invalid_assertion"),
            }
        })?;
    let passkey: Uuid = sqlx::query_scalar("SELECT id FROM passkeys WHERE credential_id = $1")
        .bind(sewa_approval::decode(&approver.id).unwrap_or_default())
        .fetch_one(&mut *transaction)
        .await?;
    if status == "draft" {
        if let Some((draft, edits)) = desired::draft(&mut transaction).await?
            && draft == id
        {
            for edit in &edits {
                desired::save(&mut transaction, edit).await?;
            }
        }
        sqlx::query("UPDATE changes SET status = 'approved', closed_at = now() WHERE id = $1")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
    }
    let hosts = serde_json::to_value(&approval.hosts).map_err(|_| internal())?;
    let assertion_value = serde_json::to_value(&assertion).map_err(|_| internal())?;
    let approval_id: Uuid = sqlx::query_scalar(
        "INSERT INTO approvals (change_id, by, passkey_id, hosts, assertion)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(id)
    .bind(&identity.login)
    .bind(passkey)
    .bind(&hosts)
    .bind(&assertion_value)
    .fetch_one(&mut *transaction)
    .await?;
    for host in &approval.hosts {
        sqlx::query(
            "INSERT INTO change_hosts (change_id, host_id, approval_id, plan_hash)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (change_id, host_id) DO UPDATE SET
                 approval_id = $3, plan_hash = $4, status = 'waiting', log = NULL,
                 reported_at = NULL",
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
                change: Some(id),
                role: None,
            },
            events::CHANGE_APPROVED,
            serde_json::json!({
                "by": identity.login,
                "approval_id": approval_id,
                "plan_hash": host.plan_hash,
            }),
        )
        .await?;
    }
    let change = changes::load(&mut transaction, id).await?;
    transaction.commit().await?;
    Ok(Json(change))
}

fn internal() -> Failure {
    Failure::new(StatusCode::INTERNAL_SERVER_ERROR, "internal")
}

/// The oldest approval a host has not reported on, as the agent gets it.
pub async fn pending(pool: &sqlx::PgPool, host: Uuid) -> sqlx::Result<Option<Signed>> {
    let row: Option<(Uuid, Uuid, serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT a.id, a.change_id, a.hosts, a.assertion
         FROM change_hosts ch JOIN approvals a ON a.id = ch.approval_id
         WHERE ch.host_id = $1 AND ch.status = 'waiting'
         ORDER BY a.created_at, a.id
         LIMIT 1",
    )
    .bind(host)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(approval_id, change_id, hosts, assertion)| {
        Some(Signed {
            approval_id,
            approval: Approval {
                change_id,
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
    let mut transaction = pool.begin().await?;
    let updated = sqlx::query(
        "UPDATE change_hosts SET status = $4, log = $5, reported_at = now()
         WHERE change_id = $1 AND host_id = $2 AND approval_id = $3 AND status = 'waiting'",
    )
    .bind(outcome.change_id)
    .bind(host)
    .bind(outcome.approval_id)
    .bind(&outcome.status)
    .bind(&log)
    .execute(&mut *transaction)
    .await?;
    if updated.rows_affected() == 1 {
        events::record_about(
            &mut transaction,
            About {
                host: Some(host),
                change: Some(outcome.change_id),
                role: None,
            },
            kind,
            serde_json::json!({
                "approval_id": outcome.approval_id,
                "plan_hash": outcome.plan_hash,
            }),
        )
        .await?;
    }
    transaction.commit().await
}
