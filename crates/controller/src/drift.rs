//! What a person can do about a host's drift, under
//! `/api/hosts/{id}/drift`. Each action records an event.
//!
//! - Revert opens a revert change: approving it applies the host's
//!   approved state again, with the plan that fixes the drift.
//! - Adopt adds an undeclared package to a role or to the host's
//!   adjustments, as an edit of the draft.
//! - Ignore hides the drift and the undeclared packages of now, until the
//!   drift changes or another package shows up.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::AppState;
use crate::changes::{self, Change};
use crate::desired::{Edit, Kind};
use crate::error::{ApiResult, Failure};
use crate::events::{self, About};
use crate::whois::Identity;

fn host_id(id: &str) -> ApiResult<Uuid> {
    id.parse()
        .map_err(|_| Failure::new(StatusCode::NOT_FOUND, "not_found"))
}

/// Locks the host, or fails with 404 `not_found`.
async fn lock(connection: &mut PgConnection, host: Uuid) -> ApiResult<()> {
    let found: Option<Uuid> = sqlx::query_scalar("SELECT id FROM hosts WHERE id = $1 FOR UPDATE")
        .bind(host)
        .fetch_optional(connection)
        .await?;
    found
        .map(|_| ())
        .ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))
}

/// `POST /api/hosts/{id}/drift/revert`: opens the host's revert change,
/// or returns the one already open. Records `drift.reverted`.
pub async fn revert(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<Change>)> {
    let host = host_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    lock(&mut transaction, host).await?;
    let open: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM changes WHERE status = 'draft' AND revert_host = $1")
            .bind(host)
            .fetch_optional(&mut *transaction)
            .await?;
    let (change, status) = match open {
        Some(change) => (change, StatusCode::OK),
        None => {
            let change: Uuid = sqlx::query_scalar(
                "INSERT INTO changes (created_by, revert_host) VALUES ($1, $2) RETURNING id",
            )
            .bind(&identity.login)
            .bind(host)
            .fetch_one(&mut *transaction)
            .await?;
            let about = About {
                host: Some(host),
                change: Some(change),
                role: None,
            };
            events::record_about(
                &mut transaction,
                about,
                events::CHANGE_CREATED,
                serde_json::json!({ "by": identity.login }),
            )
            .await?;
            events::record_about(
                &mut transaction,
                about,
                events::DRIFT_REVERTED,
                serde_json::json!({ "by": identity.login }),
            )
            .await?;
            (change, StatusCode::CREATED)
        }
    };
    let change = changes::load(&mut transaction, change).await?;
    transaction.commit().await?;
    Ok((status, Json(change)))
}

#[derive(Debug, Deserialize)]
pub struct Adopt {
    /// The package, as the package manager names it.
    pub package: String,
    /// The role to add it to, or null for this host's adjustments.
    pub role: Option<Uuid>,
}

/// `POST /api/hosts/{id}/drift/adopt`: adds an undeclared package, at
/// version `latest`, to a role or to the host's adjustments, as an edit of
/// the draft. Records `drift.adopted`. Returns the draft.
pub async fn adopt(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    Json(adopt): Json<Adopt>,
) -> ApiResult<(StatusCode, Json<Change>)> {
    let host = host_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    lock(&mut transaction, host).await?;
    let name = adopt.package.trim().to_owned();
    let version = "latest".to_owned();
    let edit = match adopt.role {
        Some(role) => Edit::RoleItemSet {
            role,
            kind: Kind::Package,
            name: name.clone(),
            version,
        },
        None => Edit::HostItemSet {
            host,
            kind: Kind::Package,
            name: name.clone(),
            version,
        },
    };
    let change = changes::add_edit_in(&mut transaction, &identity.login, &edit).await?;
    events::record_about(
        &mut transaction,
        About {
            host: Some(host),
            change: Some(change),
            role: adopt.role,
        },
        events::DRIFT_ADOPTED,
        serde_json::json!({ "by": identity.login, "package": name, "role": adopt.role }),
    )
    .await?;
    let change = changes::load(&mut transaction, change).await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(change)))
}

/// `POST /api/hosts/{id}/drift/ignore`: hides the host's drift and its
/// undeclared packages as they are now. Records `drift.ignored`.
pub async fn ignore(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let host = host_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    lock(&mut transaction, host).await?;
    let hidden: serde_json::Value = sqlx::query_scalar(
        "UPDATE hosts SET drift_ignored = jsonb_build_object(
             'plan_hash', drift_plan_hash,
             'packages', to_jsonb(COALESCE(explicit_packages, '{}'::text[])))
         WHERE id = $1
         RETURNING drift_ignored",
    )
    .bind(host)
    .fetch_one(&mut *transaction)
    .await?;
    events::record(
        &mut transaction,
        host,
        events::DRIFT_IGNORED,
        serde_json::json!({ "by": identity.login, "plan_hash": hidden["plan_hash"] }),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
