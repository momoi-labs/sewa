use std::collections::{BTreeMap, BTreeSet};

use axum::Extension;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::desired::{self, Item, Key, Kind, Spec};
use crate::error::{ApiResult, Failure};
use crate::events::{self, About};
use crate::whois::Identity;
use crate::{AppState, hosts};

const ITEM_LIMIT: usize = 500;

#[derive(Debug, Deserialize)]
pub struct NewRole {
    pub name: String,
    #[serde(default)]
    pub items: Vec<Item>,
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
    pub builtin: bool,
    pub created_at: DateTime<Utc>,
    pub items: Vec<Item>,
    pub hosts: Vec<Named>,
}

pub fn items<'a>(items: impl IntoIterator<Item = (&'a Key, &'a String)>) -> Vec<Item> {
    items
        .into_iter()
        .map(|((kind, name), version)| Item {
            kind: *kind,
            name: name.clone(),
            version: version.clone(),
            os: None,
        })
        .collect()
}

fn role_items(state: &desired::State, role: Uuid) -> Vec<Item> {
    state
        .role_items
        .get(&role)
        .into_iter()
        .flatten()
        .map(|((kind, name), spec)| Item {
            kind: *kind,
            name: name.clone(),
            version: spec.version.clone(),
            os: spec.os.clone(),
        })
        .collect()
}

fn role_hosts(state: &desired::State, role: Uuid, names: &[(Uuid, String)]) -> Vec<Named> {
    names
        .iter()
        .filter(|(host, _)| {
            state.builtin == Some(role)
                || state
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

async fn read(connection: &mut sqlx::PgConnection, only: Option<Uuid>) -> ApiResult<Vec<Role>> {
    let roles: Vec<(Uuid, String, bool, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, name, builtin, created_at FROM roles
         WHERE $1::uuid IS NULL OR id = $1
         ORDER BY builtin DESC, name",
    )
    .bind(only)
    .fetch_all(&mut *connection)
    .await?;
    let host_names: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM hosts ORDER BY name")
            .fetch_all(&mut *connection)
            .await?;
    let state = desired::State::load(&mut *connection).await?;
    Ok(roles
        .into_iter()
        .map(|(id, name, builtin, created_at)| Role {
            id,
            name,
            builtin,
            created_at,
            items: role_items(&state, id),
            hosts: role_hosts(&state, id, &host_names),
        })
        .collect())
}

fn role_id(id: &str) -> ApiResult<Uuid> {
    id.parse()
        .map_err(|_| Failure::new(StatusCode::NOT_FOUND, "not_found"))
}

pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Role>>> {
    let mut connection = state.pool.acquire().await?;
    Ok(Json(read(&mut connection, None).await?))
}

pub async fn get(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Role>> {
    let id = role_id(&id)?;
    let mut connection = state.pool.acquire().await?;
    let role = read(&mut connection, Some(id))
        .await?
        .pop()
        .ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    Ok(Json(role))
}

pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewRole>,
) -> ApiResult<impl IntoResponse> {
    if !hosts::valid_name(&new.name) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_name"));
    }
    let specs = specs(&new.items)?;
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
    insert_items(&mut transaction, id, &specs).await?;
    let items: Vec<Item> = specs.iter().map(|(key, spec)| item(key, spec)).collect();
    events::record_about(
        &mut transaction,
        About {
            role: Some(id),
            ..About::default()
        },
        events::ROLE_CREATED,
        serde_json::json!({ "name": new.name, "items": items }),
    )
    .await?;
    transaction.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(Role {
            id,
            name: new.name,
            builtin: false,
            created_at,
            items,
            hosts: Vec::new(),
        }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct SavedRole {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub added: Vec<Item>,
    pub removed: Vec<Item>,
    pub changed: Vec<Item>,
}

impl Diff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

fn item((kind, name): &Key, spec: &Spec) -> Item {
    Item {
        kind: *kind,
        name: name.clone(),
        version: spec.version.clone(),
        os: spec.os.clone(),
    }
}

pub fn diff(before: &BTreeMap<Key, Spec>, after: &BTreeMap<Key, Spec>) -> Diff {
    let mut diff = Diff::default();
    for (key, spec) in after {
        match before.get(key) {
            None => diff.added.push(item(key, spec)),
            Some(old) if old != spec => diff.changed.push(item(key, spec)),
            Some(_) => {}
        }
    }
    for (key, spec) in before {
        if !after.contains_key(key) {
            diff.removed.push(item(key, spec));
        }
    }
    diff
}

fn specs(items: &[Item]) -> ApiResult<BTreeMap<Key, Spec>> {
    if items.len() > ITEM_LIMIT {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "too_many_items"));
    }
    let mut specs = BTreeMap::new();
    for item in items {
        if !item.is_valid() {
            return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_item"));
        }
        let spec = Spec {
            version: item.version.clone(),
            os: item.os.clone(),
        };
        if specs.insert((item.kind, item.name.clone()), spec).is_some() {
            return Err(Failure::new(StatusCode::BAD_REQUEST, "duplicate_item"));
        }
    }
    Ok(specs)
}

async fn insert_items(
    connection: &mut sqlx::PgConnection,
    role: Uuid,
    specs: &BTreeMap<Key, Spec>,
) -> ApiResult<()> {
    for ((kind, name), spec) in specs {
        sqlx::query(
            "INSERT INTO role_items (role_id, kind, name, version, os) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(role)
        .bind(kind.as_str())
        .bind(name)
        .bind(&spec.version)
        .bind(&spec.os)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

pub async fn save(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    Json(saved): Json<SavedRole>,
) -> ApiResult<Json<Role>> {
    let id = role_id(&id)?;
    if !hosts::valid_name(&saved.name) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_name"));
    }
    let after = specs(&saved.items)?;
    let mut transaction = state.pool.begin().await?;
    let row: Option<(String, bool)> =
        sqlx::query_as("SELECT name, builtin FROM roles WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await?;
    let (name, builtin) = row.ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    if builtin {
        return Err(Failure::new(StatusCode::CONFLICT, "builtin_role"));
    }
    if saved.name != name {
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM roles WHERE name = $1)")
            .bind(&saved.name)
            .fetch_one(&mut *transaction)
            .await?;
        if taken {
            return Err(Failure::new(StatusCode::CONFLICT, "name_taken"));
        }
        sqlx::query("UPDATE roles SET name = $2 WHERE id = $1")
            .bind(id)
            .bind(&saved.name)
            .execute(&mut *transaction)
            .await?;
    }
    let before_rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT kind, name, version, os FROM role_items WHERE role_id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await?;
    let before: BTreeMap<Key, Spec> = before_rows
        .into_iter()
        .filter_map(|(kind, name, version, os)| {
            Some(((Kind::parse(&kind)?, name), Spec { version, os }))
        })
        .collect();
    let diff = diff(&before, &after);
    sqlx::query("DELETE FROM role_items WHERE role_id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    insert_items(&mut transaction, id, &after).await?;
    if !diff.is_empty() || saved.name != name {
        let mut data = serde_json::json!({
            "by": identity.login,
            "name": saved.name,
            "added": diff.added,
            "removed": diff.removed,
            "changed": diff.changed,
        });
        if saved.name != name {
            data["renamed_from"] = serde_json::json!(name);
        }
        events::record_about(
            &mut transaction,
            About {
                role: Some(id),
                ..About::default()
            },
            events::ROLE_SAVED,
            data,
        )
        .await?;
    }
    let role = read(&mut transaction, Some(id))
        .await?
        .pop()
        .ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    transaction.commit().await?;
    Ok(Json(role))
}

pub async fn writable(connection: &mut sqlx::PgConnection, id: Uuid) -> ApiResult<()> {
    let builtin: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM roles WHERE id = $1 AND builtin)")
            .bind(id)
            .fetch_one(connection)
            .await?;
    if builtin {
        return Err(Failure::new(StatusCode::CONFLICT, "builtin_role"));
    }
    Ok(())
}

pub async fn known(
    connection: &mut sqlx::PgConnection,
    roles: &[Uuid],
) -> ApiResult<BTreeSet<Uuid>> {
    let found: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM roles WHERE id = ANY($1) AND NOT builtin")
            .bind(roles)
            .fetch_all(connection)
            .await?;
    let wanted: BTreeSet<Uuid> = roles.iter().copied().collect();
    let found: BTreeSet<Uuid> = found.into_iter().collect();
    if found != wanted {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "unknown_role"));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_lists_added_removed_and_changed_items() {
        let key = |name: &str| (Kind::Package, name.to_owned());
        let before = BTreeMap::from([
            (key("git"), Spec::new("latest")),
            (key("jq"), Spec::new("latest")),
            (key("sl"), Spec::new("latest")),
        ]);
        let after = BTreeMap::from([
            (key("git"), Spec::new("latest")),
            (
                key("jq"),
                Spec {
                    version: "latest".to_owned(),
                    os: Some("macos".to_owned()),
                },
            ),
            (key("wget"), Spec::new("1")),
        ]);
        let diff = diff(&before, &after);
        let names = |items: &[Item]| items.iter().map(|i| i.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&diff.added), ["wget"]);
        assert_eq!(names(&diff.removed), ["sl"]);
        assert_eq!(names(&diff.changed), ["jq"]);
        assert!(super::diff(&before, &before).is_empty());
    }
}
