//! The JSON API for people, under `/api`: create a host, list the hosts,
//! and read one host, its desired state, and its events.
//! Only the Tailscale users in the admin list may call it (`require_admin`).

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiResult, Failure};
use crate::events::About;
use crate::status::{Status, status};
use crate::whois::Identity;
use crate::{AppState, desired, events, install, roles, secret, syncs};

/// How long an install token stays valid.
pub const TOKEN_LIFETIME: Duration = Duration::hours(1);

/// The Tailscale identity of the peer, from tailscaled. A request from an
/// address that is not a tailnet node gets 403 `not_tailnet`, and one that
/// tailscaled cannot answer 503 `whois_unavailable`.
pub fn identify<'a>(
    state: &'a AppState,
    request: &Request,
) -> impl std::future::Future<Output = ApiResult<crate::whois::Identity>> + Send + 'a {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(peer)| *peer);
    identify_peer(state, peer)
}

async fn identify_peer(
    state: &AppState,
    peer: Option<SocketAddr>,
) -> ApiResult<crate::whois::Identity> {
    let peer = peer.ok_or(Failure::new(StatusCode::FORBIDDEN, "not_tailnet"))?;
    match state.whois.whois(peer).await {
        Ok(Some(identity)) => Ok(identity),
        Ok(None) => Err(Failure::new(StatusCode::FORBIDDEN, "not_tailnet")),
        Err(error) => {
            tracing::warn!(
                error = format!("{error:#}"),
                "cannot ask tailscaled who calls"
            );
            Err(Failure::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "whois_unavailable",
            ))
        }
    }
}

/// Lets a request through only when its Tailscale user is in the admin
/// list. Others get 403 `not_admin`.
pub async fn require_admin(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    match identify(&state, &request).await {
        Ok(identity) if state.admins.contains(&identity.login) => {
            request.extensions_mut().insert(identity);
            next.run(request).await
        }
        Ok(_) => Failure::new(StatusCode::FORBIDDEN, "not_admin").into_response(),
        Err(failure) => failure.into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct NewHost {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct Agent {
    pub version: Option<String>,
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub user: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    /// `linux` or `macos`, as the agent reports it; null until it does.
    pub os: Option<String>,
    pub status: Status,
    pub created_at: DateTime<Utc>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub enrolled_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    /// What the agent last described; null until it sends a description.
    pub agent: Option<Agent>,
}

#[derive(Debug, Serialize)]
pub struct Created {
    pub host: Host,
    /// The one-line command that installs the agent and enrolls the host.
    /// It holds the install token, which the controller does not keep.
    pub install_command: String,
    pub token_expires_at: DateTime<Utc>,
}

/// A name of 1 to 63 letters, digits, `-`, `_`, or `.`.
pub fn valid_name(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

async fn issue_token(
    connection: &mut sqlx::PgConnection,
    host: Uuid,
) -> sqlx::Result<(String, DateTime<Utc>)> {
    let token = secret::token();
    let expires_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO install_tokens (token_hash, host_id, expires_at)
         VALUES ($1, $2, now() + make_interval(secs => $3))
         RETURNING expires_at",
    )
    .bind(secret::hash(&token))
    .bind(host)
    .bind(TOKEN_LIFETIME.num_seconds() as f64)
    .fetch_one(connection)
    .await?;
    Ok((token, expires_at))
}

/// `POST /api/hosts`: stores a host as waiting and returns its install
/// command with a fresh single-use token.
pub async fn create(
    State(state): State<AppState>,
    Json(new): Json<NewHost>,
) -> ApiResult<impl IntoResponse> {
    if !valid_name(&new.name) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_name"));
    }
    let mut transaction = state.pool.begin().await?;
    let row: Option<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "INSERT INTO hosts (name) VALUES ($1)
         ON CONFLICT (name) DO NOTHING
         RETURNING id, created_at",
    )
    .bind(&new.name)
    .fetch_optional(&mut *transaction)
    .await?;
    let (id, created_at) = row.ok_or(Failure::new(StatusCode::CONFLICT, "name_taken"))?;
    let (token, expires_at) = issue_token(&mut transaction, id).await?;
    events::record(
        &mut transaction,
        id,
        events::HOST_CREATED,
        serde_json::json!({ "name": new.name }),
    )
    .await?;
    transaction.commit().await?;
    tracing::info!(host = %id, "host created");
    let created = Created {
        host: Host {
            id,
            name: new.name,
            os: None,
            status: Status::Waiting,
            created_at,
            disabled_at: None,
            enrolled_at: None,
            last_seen_at: None,
            agent: None,
        },
        install_command: install::command(&state.public_url, &token),
        token_expires_at: expires_at,
    };
    Ok((StatusCode::CREATED, Json(created)))
}

type Row = (
    Uuid,
    String,
    Option<String>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// A query of [`Row`]s: `SELECT <its columns> FROM hosts` and the rest.
macro_rules! select_hosts {
    ($rest:literal) => {
        concat!(
            "SELECT id, name, os, created_at, disabled_at, enrolled_at, last_seen_at, agent_instance_uid,
                    agent_version, agent_hostname, agent_os, agent_arch, agent_user
             FROM hosts ",
            $rest
        )
    };
}

fn host(row: Row, now: DateTime<Utc>) -> Host {
    let (
        id,
        name,
        os,
        created_at,
        disabled_at,
        enrolled_at,
        last_seen_at,
        instance,
        version,
        hostname,
        agent_os,
        arch,
        user,
    ) = row;
    Host {
        id,
        name,
        os,
        status: status(disabled_at, enrolled_at, last_seen_at, now),
        created_at,
        disabled_at,
        enrolled_at,
        last_seen_at,
        agent: instance.map(|_| Agent {
            version,
            hostname,
            os: agent_os,
            arch,
            user,
        }),
    }
}

/// A host in the list, with the roles it has besides the built-in one.
#[derive(Debug, Serialize)]
pub struct Listed {
    #[serde(flatten)]
    pub host: Host,
    pub roles: Vec<roles::Named>,
}

/// `GET /api/hosts`: every host by name, with its status and its roles.
/// The built-in role is left out: every host has it.
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Listed>>> {
    let rows: Vec<Row> = sqlx::query_as(select_hosts!("ORDER BY name"))
        .fetch_all(&state.pool)
        .await?;
    let pairs: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT host_roles.host_id, roles.id, roles.name
         FROM host_roles JOIN roles ON roles.id = host_roles.role_id
         WHERE NOT roles.builtin
         ORDER BY roles.name",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut roles_of: std::collections::HashMap<Uuid, Vec<roles::Named>> =
        std::collections::HashMap::new();
    for (host, id, name) in pairs {
        roles_of
            .entry(host)
            .or_default()
            .push(roles::Named { id, name });
    }
    let now = Utc::now();
    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let host = host(row, now);
                let roles = roles_of.remove(&host.id).unwrap_or_default();
                Listed { host, roles }
            })
            .collect(),
    ))
}

/// The host id of a path. One that is not a UUID names no host.
fn host_id(id: &str) -> ApiResult<Uuid> {
    id.parse()
        .map_err(|_| Failure::new(StatusCode::NOT_FOUND, "not_found"))
}

/// `GET /api/hosts/{id}`: one host, or 404 `not_found`.
pub async fn get(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Host>> {
    let id = host_id(&id)?;
    let row: Option<Row> = sqlx::query_as(select_hosts!("WHERE id = $1"))
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    let row = row.ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    Ok(Json(host(row, Utc::now())))
}

async fn lock(connection: &mut sqlx::PgConnection, id: Uuid) -> ApiResult<bool> {
    let disabled: Option<bool> =
        sqlx::query_scalar("SELECT disabled_at IS NOT NULL FROM hosts WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(connection)
            .await?;
    disabled.ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))
}

async fn read(connection: &mut sqlx::PgConnection, id: Uuid) -> ApiResult<Host> {
    let row: Row = sqlx::query_as(select_hosts!("WHERE id = $1"))
        .bind(id)
        .fetch_one(connection)
        .await?;
    Ok(host(row, Utc::now()))
}

pub async fn disable(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> ApiResult<Json<Host>> {
    let id = host_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    if lock(&mut transaction, id).await? {
        return Err(Failure::new(StatusCode::CONFLICT, "already_disabled"));
    }
    sqlx::query("UPDATE hosts SET disabled_at = now(), connected = false WHERE id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    let revoked = sqlx::query(
        "UPDATE agent_credentials SET revoked_at = now()
         WHERE host_id = $1 AND revoked_at IS NULL",
    )
    .bind(id)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    sqlx::query(
        "UPDATE install_tokens SET expires_at = now()
         WHERE host_id = $1 AND used_at IS NULL AND expires_at > now()",
    )
    .bind(id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE sync_hosts SET status = 'skipped' WHERE host_id = $1 AND status = 'picked'",
    )
    .bind(id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE sync_hosts SET status = 'failed', reported_at = now(), received_at = now(),
             log = 'The host was disabled before its agent reported.'
         WHERE host_id = $1 AND status = 'waiting'",
    )
    .bind(id)
    .execute(&mut *transaction)
    .await?;
    events::record(
        &mut transaction,
        id,
        events::HOST_DISABLED,
        serde_json::json!({ "by": identity.login, "revoked_credentials": revoked }),
    )
    .await?;
    let host = read(&mut transaction, id).await?;
    transaction.commit().await?;
    tracing::info!(host = %id, "host disabled");
    Ok(Json(host))
}

pub async fn enable(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
) -> ApiResult<Json<Created>> {
    let id = host_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    if !lock(&mut transaction, id).await? {
        return Err(Failure::new(StatusCode::CONFLICT, "not_disabled"));
    }
    sqlx::query("UPDATE hosts SET disabled_at = NULL, enrolled_at = NULL WHERE id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    let (token, expires_at) = issue_token(&mut transaction, id).await?;
    events::record(
        &mut transaction,
        id,
        events::HOST_ENABLED,
        serde_json::json!({ "by": identity.login }),
    )
    .await?;
    let host = read(&mut transaction, id).await?;
    transaction.commit().await?;
    tracing::info!(host = %id, "host enabled");
    Ok(Json(Created {
        host,
        install_command: install::command(&state.public_url, &token),
        token_expires_at: expires_at,
    }))
}

pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = host_id(&id)?;
    let mut transaction = state.pool.begin().await?;
    lock(&mut transaction, id).await?;
    sqlx::query("SELECT set_config('sewa.deleting_host', 'on', true)")
        .execute(&mut *transaction)
        .await?;
    let syncs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM syncs
         WHERE id IN (SELECT sync_id FROM sync_hosts WHERE host_id = $1)
         ORDER BY id FOR UPDATE",
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await?;
    macro_rules! forget {
        ($($table:literal),*) => {$(
            sqlx::query(concat!("DELETE FROM ", $table, " WHERE host_id = $1"))
                .bind(id)
                .execute(&mut *transaction)
                .await?;
        )*};
    }
    forget!(
        "events",
        "sync_hosts",
        "host_plans",
        "host_roles",
        "host_adjustments",
        "install_tokens",
        "agent_credentials"
    );
    let alone: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM syncs
         WHERE id = ANY($1) AND NOT EXISTS (SELECT 1 FROM sync_hosts WHERE sync_id = syncs.id)",
    )
    .bind(&syncs)
    .fetch_all(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM events WHERE sync_id = ANY($1)")
        .bind(&alone)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM approvals WHERE sync_id = ANY($1)")
        .bind(&alone)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM syncs WHERE id = ANY($1)")
        .bind(&alone)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM hosts WHERE id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    tracing::info!(host = %id, "host deleted");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct Event {
    pub id: i64,
    pub kind: String,
    pub at: DateTime<Utc>,
    pub data: serde_json::Value,
}

/// How many events `GET /api/hosts/{id}/events` returns.
const EVENT_LIMIT: i64 = 100;

/// `GET /api/hosts/{id}/events`: the host's latest events, newest first,
/// or 404 `not_found`.
pub async fn events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<Event>>> {
    let id = host_id(&id)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM hosts WHERE id = $1)")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    if !exists {
        return Err(Failure::new(StatusCode::NOT_FOUND, "not_found"));
    }
    let rows: Vec<(i64, String, DateTime<Utc>, serde_json::Value)> = sqlx::query_as(
        "SELECT id, kind, at, data FROM events WHERE host_id = $1 ORDER BY at DESC, id DESC LIMIT $2",
    )
    .bind(id)
    .bind(EVENT_LIMIT)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, kind, at, data)| Event { id, kind, at, data })
            .collect(),
    ))
}

#[derive(Debug, Serialize)]
pub struct Adjustment {
    pub kind: desired::Kind,
    pub name: String,
    #[serde(flatten)]
    pub adjustment: desired::Adjustment,
}

#[derive(Debug, Serialize)]
pub struct Drift {
    /// `in_sync` (exit code 0), `drift` (2), or `error` (1 or another).
    pub status: &'static str,
    /// Ignore hid this drift. It shows again when the drift changes.
    pub ignored: bool,
    #[serde(flatten)]
    pub plan: syncs::Plan,
}

#[derive(Debug, Serialize)]
pub struct Undeclared {
    pub manager: String,
    /// The explicitly installed packages that the host's approved items do
    /// not declare, by name.
    pub packages: Vec<String>,
    /// Undeclared packages that Ignore hid, left out of `packages`.
    pub ignored_count: usize,
    pub explicit_count: usize,
    pub reported_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct HostState {
    /// The roles an admin gave the host. Every host also gets the built-in
    /// role, which is not in this list.
    pub roles: Vec<roles::Named>,
    pub adjustments: Vec<Adjustment>,
    /// The host's items: the built-in role's, its roles' for its OS, and
    /// its adjustments.
    pub items: Vec<desired::Item>,
    /// The config the host gets for them.
    pub config: String,
    /// The health status the agent last reported.
    pub health: Option<String>,
    /// The last plan of the desired state, or null before the first.
    pub drift: Option<Drift>,
    pub undeclared: Option<Undeclared>,
}

fn named_roles(state: &desired::State, host: Uuid) -> Vec<roles::Named> {
    state
        .roles_of(host)
        .into_iter()
        .map(|(id, name)| roles::Named { id, name })
        .collect()
}

fn adjustments(state: &desired::State, host: Uuid) -> Vec<Adjustment> {
    state
        .adjustments
        .get(&host)
        .into_iter()
        .flatten()
        .map(|((kind, name), adjustment)| Adjustment {
            kind: *kind,
            name: name.clone(),
            adjustment: adjustment.clone(),
        })
        .collect()
}

/// `GET /api/hosts/{id}/state`: the host's roles, adjustments, items,
/// drift, and undeclared packages, or 404 `not_found`.
pub async fn state(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<HostState>> {
    let id = host_id(&id)?;
    let mut connection = state.pool.acquire().await?;
    Ok(Json(host_state(&mut connection, id).await?))
}

async fn host_state(connection: &mut sqlx::PgConnection, id: Uuid) -> ApiResult<HostState> {
    type Explicit = (
        Option<String>,
        Option<String>,
        Option<Vec<String>>,
        Option<DateTime<Utc>>,
        Option<serde_json::Value>,
    );
    let row: Option<Explicit> = sqlx::query_as(
        "SELECT health, explicit_manager, explicit_packages, explicit_reported_at, drift_ignored
         FROM hosts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;
    let (health, manager, explicit, reported_at, ignored) =
        row.ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    let ignored = ignored.unwrap_or_default();
    let ignored_plan = ignored["plan_hash"].as_str();
    let ignored_packages: Vec<&str> = ignored["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let current = desired::State::load(&mut *connection).await?;
    let items = current.host_items(id);
    let config = desired::render(&items);
    let plan: Option<_> = sqlx::query_as(syncs::SELECT_PLAN)
        .bind(id)
        .bind(desired::CURRENT)
        .fetch_optional(&mut *connection)
        .await?;
    let drift = plan.map(|row| {
        let plan = syncs::plan(row, Some(&desired::hash(&config)));
        Drift {
            status: match plan.exit_code {
                0 => "in_sync",
                2 => "drift",
                _ => "error",
            },
            ignored: plan.exit_code == 2 && ignored_plan == Some(plan.plan_hash.as_str()),
            plan,
        }
    });
    let undeclared = match (manager, explicit, reported_at) {
        (Some(manager), Some(explicit), Some(reported_at)) => {
            let declared = desired::declared_packages(&items, &manager);
            let (hidden, mut packages): (Vec<String>, Vec<String>) = explicit
                .iter()
                .filter(|package| !declared.contains(*package))
                .cloned()
                .partition(|package| ignored_packages.contains(&package.as_str()));
            packages.sort();
            Some(Undeclared {
                manager,
                packages,
                ignored_count: hidden.len(),
                explicit_count: explicit.len(),
                reported_at,
            })
        }
        _ => None,
    };
    Ok(HostState {
        roles: named_roles(&current, id),
        adjustments: adjustments(&current, id),
        items: roles::items(&items),
        config,
        health,
        drift,
        undeclared,
    })
}

#[derive(Debug, Deserialize)]
pub struct Edits {
    pub edits: Vec<desired::Edit>,
}

/// The most edits one call takes.
const EDIT_LIMIT: usize = 500;

/// `POST /api/hosts/{id}/edits`: changes the host's roles and adjustments
/// at once, and returns its state. 400 `invalid_edit`, `too_many_edits`,
/// or `unknown_role` (the built-in role counts as unknown: every host has
/// it). Records `host.roles_set` and `host.items_set`. Hosts get the
/// change at their next sync.
pub async fn edit(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Path(id): Path<String>,
    Json(edits): Json<Edits>,
) -> ApiResult<Json<HostState>> {
    let id = host_id(&id)?;
    let edits = edits.edits;
    if edits.len() > EDIT_LIMIT {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "too_many_edits"));
    }
    if !edits.iter().all(desired::Edit::is_valid) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_edit"));
    }
    let mut transaction = state.pool.begin().await?;
    let found: Option<Uuid> = sqlx::query_scalar("SELECT id FROM hosts WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?;
    found.ok_or(Failure::new(StatusCode::NOT_FOUND, "not_found"))?;
    let named: Vec<Uuid> = edits
        .iter()
        .filter_map(|edit| match edit {
            desired::Edit::HostRoleAdd { role } | desired::Edit::HostRoleRemove { role } => {
                Some(*role)
            }
            _ => None,
        })
        .collect();
    roles::known(&mut transaction, &named).await?;
    let before = desired::State::load(&mut transaction).await?.roles_of(id);
    let mut items = Vec::new();
    for edit in &edits {
        desired::save(&mut transaction, id, edit).await?;
        if !matches!(
            edit,
            desired::Edit::HostRoleAdd { .. } | desired::Edit::HostRoleRemove { .. }
        ) {
            items.push(edit);
        }
    }
    let after = desired::State::load(&mut transaction).await?.roles_of(id);
    let names = |from: &[(Uuid, String)], to: &[(Uuid, String)]| -> Vec<String> {
        from.iter()
            .filter(|role| !to.contains(role))
            .map(|(_, name)| name.clone())
            .collect()
    };
    let (added, removed) = (names(&after, &before), names(&before, &after));
    let about = About {
        host: Some(id),
        sync: None,
        role: None,
    };
    if !added.is_empty() || !removed.is_empty() {
        events::record_about(
            &mut transaction,
            about,
            events::HOST_ROLES_SET,
            serde_json::json!({ "by": identity.login, "added": added, "removed": removed }),
        )
        .await?;
    }
    if !items.is_empty() {
        events::record_about(
            &mut transaction,
            about,
            events::HOST_ITEMS_SET,
            serde_json::json!({ "by": identity.login, "edits": items }),
        )
        .await?;
    }
    let host_state = host_state(&mut transaction, id).await?;
    transaction.commit().await?;
    Ok(Json(host_state))
}

#[cfg(test)]
mod tests {
    use super::valid_name;

    #[test]
    fn names() {
        assert!(valid_name("build-01.lab"));
        assert!(!valid_name(""));
        assert!(!valid_name("two words"));
        assert!(!valid_name(&"a".repeat(64)));
    }
}
