use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::desired::{self, Edit, Item, Kind};
use crate::error::{ApiResult, Failure};
use crate::events::{self, About};
use crate::whois::Identity;
use crate::{AppState, roles};

fn host_id(id: &str) -> ApiResult<Uuid> {
    id.parse()
        .map_err(|_| Failure::new(StatusCode::NOT_FOUND, "not_found"))
}

async fn lock(connection: &mut PgConnection, host: Uuid) -> ApiResult<()> {
    let found: Option<Uuid> = sqlx::query_scalar("SELECT id FROM hosts WHERE id = $1 FOR UPDATE")
        .bind(host)
        .fetch_optional(connection)
        .await?;
    found
        .map(|_| ())
        .ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))
}

#[derive(Debug, Deserialize)]
pub struct Adopt {
    pub package: String,
    pub role: Option<Uuid>,
}

pub async fn adopt(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    Json(adopt): Json<Adopt>,
) -> ApiResult<StatusCode> {
    let host = host_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    lock(&mut transaction, host).await?;
    let name = adopt.package.trim().to_owned();
    let item = Item {
        kind: Kind::Package,
        name: name.clone(),
        version: "latest".to_owned(),
        os: None,
    };
    if !item.is_valid() {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_item"));
    }
    match adopt.role {
        Some(role) => {
            roles::writable(&mut transaction, role).await?;
            roles::known(&mut transaction, &[role]).await?;
            sqlx::query(
                "INSERT INTO role_items (role_id, kind, name, version)
                 VALUES ($1, 'package', $2, 'latest')
                 ON CONFLICT DO NOTHING",
            )
            .bind(role)
            .bind(&name)
            .execute(&mut *transaction)
            .await?;
        }
        None => {
            let edit = Edit::HostItemSet {
                kind: Kind::Package,
                name: name.clone(),
                version: item.version,
            };
            desired::save(&mut transaction, host, &edit).await?;
        }
    }
    events::record_about(
        &mut transaction,
        About {
            host: Some(host),
            sync: None,
            role: adopt.role,
        },
        events::DRIFT_ADOPTED,
        serde_json::json!({ "by": identity.login, "package": name, "role": adopt.role }),
    )
    .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

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
