//! The JSON API for people, under `/api`: create a host and list the hosts.
//! Only the Tailscale users in the admin list may call it (`require_admin`).

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiResult, Failure};
use crate::status::{Status, status};
use crate::{AppState, events, install, secret};

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
    request: Request,
    next: Next,
) -> Response {
    match identify(&state, &request).await {
        Ok(identity) if state.admins.contains(&identity.login) => next.run(request).await,
        Ok(_) => Failure::new(StatusCode::FORBIDDEN, "not_admin").into_response(),
        Err(failure) => failure.into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct NewHost {
    pub name: String,
    pub os: String,
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
    pub os: String,
    pub status: Status,
    pub created_at: DateTime<Utc>,
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
fn valid_name(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
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
    if !matches!(new.os.as_str(), "linux" | "macos") {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_os"));
    }
    let mut transaction = state.pool.begin().await?;
    let row: Option<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "INSERT INTO hosts (name, os) VALUES ($1, $2)
         ON CONFLICT (name) DO NOTHING
         RETURNING id, created_at",
    )
    .bind(&new.name)
    .bind(&new.os)
    .fetch_optional(&mut *transaction)
    .await?;
    let (id, created_at) = row.ok_or(Failure::new(StatusCode::CONFLICT, "name_taken"))?;
    let token = secret::token();
    let expires_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO install_tokens (token_hash, host_id, expires_at)
         VALUES ($1, $2, now() + make_interval(secs => $3))
         RETURNING expires_at",
    )
    .bind(secret::hash(&token))
    .bind(id)
    .bind(TOKEN_LIFETIME.num_seconds() as f64)
    .fetch_one(&mut *transaction)
    .await?;
    events::record(
        &mut transaction,
        id,
        events::HOST_CREATED,
        serde_json::json!({ "name": new.name, "os": new.os }),
    )
    .await?;
    transaction.commit().await?;
    tracing::info!(host = %id, "host created");
    let created = Created {
        host: Host {
            id,
            name: new.name,
            os: new.os,
            status: Status::Waiting,
            created_at,
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
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<Uuid>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// `GET /api/hosts`: every host by name, with its status.
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Host>>> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, name, os, created_at, enrolled_at, last_seen_at, agent_instance_uid,
                agent_version, agent_hostname, agent_os, agent_arch, agent_user
         FROM hosts ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await?;
    let now = Utc::now();
    let hosts = rows
        .into_iter()
        .map(
            |(
                id,
                name,
                os,
                created_at,
                enrolled_at,
                last_seen_at,
                instance,
                version,
                hostname,
                agent_os,
                arch,
                user,
            )| Host {
                id,
                name,
                os,
                status: status(enrolled_at, last_seen_at, now),
                created_at,
                enrolled_at,
                last_seen_at,
                agent: instance.map(|_| Agent {
                    version,
                    hostname,
                    os: agent_os,
                    arch,
                    user,
                }),
            },
        )
        .collect();
    Ok(Json(hosts))
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
