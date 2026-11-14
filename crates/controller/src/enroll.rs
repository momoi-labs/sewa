//! `POST /v1/enroll`: an agent trades its install token for a credential.
//!
//! Adapted from pg-yard's `crates/cell/src/agent.rs`. The request must come
//! from a tailnet node, which tailscaled confirms.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use serde::Deserialize;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::{ApiResult, Failure};
use crate::hosts::identify;
use crate::{AppState, events, secret};

#[derive(Deserialize)]
struct Enroll {
    token: String,
}

/// The largest enroll body read.
const BODY_LIMIT: usize = 4 * 1024;

fn token_invalid() -> Failure {
    Failure::new(StatusCode::UNAUTHORIZED, "token_invalid")
}

/// One transaction: the host first, then its token and credentials. Either
/// the token is spent and the new credential exists, or nothing changed.
/// Earlier credentials of the host are revoked. The answer is the bare
/// credential as text.
pub async fn enroll(
    State(state): State<AppState>,
    request: Request,
) -> ApiResult<impl IntoResponse> {
    let identity = identify(&state, &request).await?;
    let body = axum::body::to_bytes(request.into_body(), BODY_LIMIT)
        .await
        .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_body"))?;
    let Enroll { token } = serde_json::from_slice(&body)
        .map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_body"))?;
    let token_hash = secret::hash(&token);
    // Unlocked, only to find which host to lock.
    let host: Option<Uuid> =
        sqlx::query_scalar("SELECT host_id FROM install_tokens WHERE token_hash = $1")
            .bind(&token_hash)
            .fetch_optional(&state.pool)
            .await?;
    let host = host.ok_or_else(token_invalid)?;

    let mut transaction = state.pool.begin().await?;
    sqlx::query("SELECT 1 FROM hosts WHERE id = $1 FOR UPDATE")
        .bind(host)
        .execute(&mut *transaction)
        .await?;
    // clock_timestamp(), not now(): now() is when the transaction began, and
    // this call may have waited on the host's lock past the expiry.
    let spent = sqlx::query(
        "UPDATE install_tokens SET used_at = now()
         WHERE token_hash = $1 AND host_id = $2 AND used_at IS NULL
           AND expires_at > clock_timestamp()",
    )
    .bind(&token_hash)
    .bind(host)
    .execute(&mut *transaction)
    .await?;
    if spent.rows_affected() != 1 {
        return Err(token_invalid());
    }
    sqlx::query(
        "UPDATE agent_credentials SET revoked_at = now()
         WHERE host_id = $1 AND revoked_at IS NULL",
    )
    .bind(host)
    .execute(&mut *transaction)
    .await?;
    let credential = secret::token();
    sqlx::query("INSERT INTO agent_credentials (credential_hash, host_id) VALUES ($1, $2)")
        .bind(secret::hash(&credential))
        .bind(host)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE hosts SET enrolled_at = now() WHERE id = $1")
        .bind(host)
        .execute(&mut *transaction)
        .await?;
    events::record(
        &mut transaction,
        host,
        events::HOST_ENROLLED,
        serde_json::json!({ "node": identity.node }),
    )
    .await?;
    transaction.commit().await?;
    tracing::info!(%host, "host enrolled");

    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        format!("{credential}\n"),
    ))
}

/// The credential in an `Authorization: Bearer` header.
pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
}

/// The host a live credential belongs to. The database keeps only the
/// credential's SHA-256, and the stored hash is compared again in constant
/// time.
pub async fn authenticate(pool: &sqlx::PgPool, credential: &str) -> sqlx::Result<Option<Uuid>> {
    let hash = secret::hash(credential);
    let row: Option<(Vec<u8>, Uuid)> = sqlx::query_as(
        "SELECT credential_hash, host_id FROM agent_credentials
         WHERE credential_hash = $1 AND revoked_at IS NULL",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await?;
    Ok(row
        .filter(|(stored, _)| bool::from(stored.ct_eq(&hash)))
        .map(|(_, host)| host))
}
