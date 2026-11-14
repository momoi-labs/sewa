//! `/api/roles`: create roles and read them with their items and hosts.
//! Items and hosts change only through edits of a change (`changes.rs`).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::desired::{self, Item, Key};
use crate::error::{ApiResult, Failure};
use crate::events::{self, About};
use crate::{AppState, hosts};

#[derive(Debug, Deserialize)]
pub struct NewRole {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Named {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct Role {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    /// The approved items.
    pub items: Vec<Item>,
    /// The items with the draft applied, or null without a draft.
    pub proposed_items: Option<Vec<Item>>,
    /// The hosts that have the role, by name.
    pub hosts: Vec<Named>,
    pub proposed_hosts: Option<Vec<Named>>,
}

pub fn items<'a>(items: impl IntoIterator<Item = (&'a Key, &'a String)>) -> Vec<Item> {
    items
        .into_iter()
        .map(|((kind, name), version)| Item {
            kind: *kind,
            name: name.clone(),
            version: version.clone(),
        })
        .collect()
}

fn role_items(state: &desired::State, role: Uuid) -> Vec<Item> {
    items(state.role_items.get(&role).into_iter().flatten())
}

fn role_hosts(state: &desired::State, role: Uuid, names: &[(Uuid, String)]) -> Vec<Named> {
    names
        .iter()
        .filter(|(host, _)| {
            state
                .host_roles
                .get(host)
                .is_some_and(|roles| roles.contains(&role))
        })
        .map(|(id, name)| Named {
            id: *id,
            name: name.clone(),
        })
        .collect()
}

async fn read(state: &AppState, only: Option<Uuid>) -> ApiResult<Vec<Role>> {
    let mut connection = state.pool.acquire().await?;
    let roles: Vec<(Uuid, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, name, created_at FROM roles WHERE $1::uuid IS NULL OR id = $1 ORDER BY name",
    )
    .bind(only)
    .fetch_all(&mut *connection)
    .await?;
    let host_names: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM hosts ORDER BY name")
            .fetch_all(&mut *connection)
            .await?;
    let (approved, proposed) = desired::states(&mut connection).await?;
    Ok(roles
        .into_iter()
        .map(|(id, name, created_at)| Role {
            id,
            name,
            created_at,
            items: role_items(&approved, id),
            proposed_items: proposed
                .as_ref()
                .map(|(_, proposed)| role_items(proposed, id)),
            hosts: role_hosts(&approved, id, &host_names),
            proposed_hosts: proposed
                .as_ref()
                .map(|(_, proposed)| role_hosts(proposed, id, &host_names)),
        })
        .collect())
}

/// `GET /api/roles`: every role by name.
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Role>>> {
    Ok(Json(read(&state, None).await?))
}

/// `GET /api/roles/{id}`: one role, or 404 `not_found`.
pub async fn get(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Role>> {
    let id: Uuid = id
        .parse()
        .map_err(|_| Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    let role = read(&state, Some(id))
        .await?
        .pop()
        .ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    Ok(Json(role))
}

/// `POST /api/roles`: creates an empty role. An empty role changes no
/// host, so it needs no change.
pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewRole>,
) -> ApiResult<impl IntoResponse> {
    if !hosts::valid_name(&new.name) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_name"));
    }
    let mut transaction = state.pool.begin().await?;
    let row: Option<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "INSERT INTO roles (name) VALUES ($1)
         ON CONFLICT (name) DO NOTHING
         RETURNING id, created_at",
    )
    .bind(&new.name)
    .fetch_optional(&mut *transaction)
    .await?;
    let (id, created_at) = row.ok_or(Failure::new(StatusCode::CONFLICT, "name_taken"))?;
    events::record_about(
        &mut transaction,
        About {
            role: Some(id),
            ..About::default()
        },
        events::ROLE_CREATED,
        serde_json::json!({ "name": new.name }),
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(Role {
            id,
            name: new.name,
            created_at,
            items: Vec::new(),
            proposed_items: None,
            hosts: Vec::new(),
            proposed_hosts: None,
        }),
    ))
}
