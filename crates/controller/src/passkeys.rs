//! `/api/passkeys`: the admins' passkeys, which sign approvals. The
//! browser creates each one with WebAuthn; the controller keeps its
//! public key. The install script pins every passkey into the agent.
//!
//! The relying party is the controller's public URL: its host is the
//! relying party id and its origin is the web app's origin.

use axum::extract::State;
use axum::http::StatusCode;
use axum::{Extension, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sewa_approval::{Approver, Pinned, Rejection};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::AppState;
use crate::error::{ApiResult, Failure};
use crate::events::{self, About};
use crate::whois::Identity;

/// How long a registration challenge stays valid, in seconds.
const CHALLENGE_LIFETIME: i64 = 300;

/// The relying party id and the origin of a public URL such as
/// `https://sewa.example.com:8443`: `sewa.example.com` and the URL itself.
pub fn relying_party(public_url: &str) -> (String, String) {
    let authority = public_url
        .split_once("://")
        .map_or(public_url, |(_, rest)| rest);
    let authority = authority.split('/').next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    (host.to_owned(), public_url.trim_end_matches('/').to_owned())
}

#[derive(Debug, Serialize)]
pub struct Passkey {
    pub id: Uuid,
    pub login: String,
    pub name: String,
    /// Unpadded base64url.
    pub credential_id: String,
    pub created_at: DateTime<Utc>,
}

type PasskeyRow = (Uuid, String, String, Vec<u8>, DateTime<Utc>);

fn passkey(row: PasskeyRow) -> Passkey {
    let (id, login, name, credential_id, created_at) = row;
    Passkey {
        id,
        login,
        name,
        credential_id: sewa_approval::encode(&credential_id),
        created_at,
    }
}

/// `GET /api/passkeys`: every admin's passkeys, oldest first. They are all
/// approver keys.
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Passkey>>> {
    let rows: Vec<PasskeyRow> = sqlx::query_as(
        "SELECT id, login, name, credential_id, created_at FROM passkeys ORDER BY created_at, id",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows.into_iter().map(passkey).collect()))
}

/// The keys an agent trusts: every passkey, with the relying party.
pub async fn pinned(connection: &mut PgConnection, public_url: &str) -> sqlx::Result<Pinned> {
    keys(connection, public_url, None).await
}

/// The passkeys of one login, or of everyone, as approver keys.
pub async fn keys(
    connection: &mut PgConnection,
    public_url: &str,
    login: Option<&str>,
) -> sqlx::Result<Pinned> {
    let rows: Vec<(Vec<u8>, Vec<u8>, String)> = sqlx::query_as(
        "SELECT credential_id, public_key, login FROM passkeys
         WHERE $1::text IS NULL OR login = $1
         ORDER BY created_at, id",
    )
    .bind(login)
    .fetch_all(connection)
    .await?;
    let (rp_id, origin) = relying_party(public_url);
    Ok(Pinned {
        rp_id,
        origin,
        keys: rows
            .into_iter()
            .map(|(id, public_key, login)| Approver {
                id: sewa_approval::encode(&id),
                public_key: sewa_approval::encode(&public_key),
                login,
            })
            .collect(),
    })
}

#[derive(Debug, Serialize)]
pub struct RelyingParty {
    pub id: String,
    pub name: &'static str,
}

#[derive(Debug, Serialize)]
pub struct User {
    /// Unpadded base64url: the first 16 bytes of the login's SHA-256.
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct Options {
    /// Unpadded base64url.
    pub challenge: String,
    pub rp: RelyingParty,
    pub user: User,
    /// The COSE algorithm the controller accepts: ES256.
    pub algorithm: i64,
    /// The signed-in user's credential ids, so the browser does not
    /// register the same authenticator twice.
    pub exclude_credentials: Vec<String>,
}

/// `POST /api/passkeys/options`: a fresh challenge for registering a
/// passkey, valid for five minutes.
pub async fn options(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
) -> ApiResult<Json<Options>> {
    let mut challenge = [0u8; 32];
    getrandom::fill(&mut challenge).expect("the operating system has no random source");
    let mut transaction = state.pool.begin().await?;
    sqlx::query(
        "DELETE FROM passkey_challenges WHERE created_at < now() - make_interval(secs => $1)",
    )
    .bind(CHALLENGE_LIFETIME as f64)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("INSERT INTO passkey_challenges (challenge, login) VALUES ($1, $2)")
        .bind(&challenge[..])
        .bind(&identity.login)
        .execute(&mut *transaction)
        .await?;
    let mine = keys(&mut transaction, &state.public_url, Some(&identity.login)).await?;
    transaction.commit().await?;
    let (rp_id, _) = relying_party(&state.public_url);
    Ok(Json(Options {
        challenge: sewa_approval::encode(&challenge),
        rp: RelyingParty {
            id: rp_id,
            name: "sewa",
        },
        user: User {
            id: sewa_approval::encode(&Sha256::digest(identity.login.as_bytes())[..16]),
            name: identity.login,
        },
        algorithm: sewa_approval::ES256,
        exclude_credentials: mine.keys.into_iter().map(|key| key.id).collect(),
    }))
}

/// A new passkey, as the browser created it. Bytes are unpadded base64url.
#[derive(Debug, Deserialize)]
pub struct Registration {
    pub name: String,
    pub credential_id: String,
    /// `getPublicKey()`: SubjectPublicKeyInfo DER.
    pub public_key: String,
    /// `getPublicKeyAlgorithm()`.
    pub algorithm: i64,
    /// `getAuthenticatorData()`.
    pub authenticator_data: String,
    pub client_data_json: String,
}

fn invalid() -> Failure {
    Failure::new(StatusCode::BAD_REQUEST, "invalid_passkey")
}

/// `POST /api/passkeys`: keeps a new passkey. Its client data must carry a
/// challenge from `options` for the same user, and the origin; its
/// authenticator data the relying party and a present user. Only ES256
/// keys are accepted.
pub async fn register(
    State(state): State<AppState>,
    Extension(identity): Extension<Identity>,
    Json(registration): Json<Registration>,
) -> ApiResult<(StatusCode, Json<Passkey>)> {
    let name = registration.name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_name"));
    }
    if registration.algorithm != sewa_approval::ES256 {
        return Err(Failure::new(
            StatusCode::BAD_REQUEST,
            "unsupported_algorithm",
        ));
    }
    sewa_approval::public_key(&registration.public_key).map_err(|_| invalid())?;
    let decode = |text: &str| sewa_approval::decode(text).map_err(|_| invalid());
    let credential_id = decode(&registration.credential_id)?;
    let public_key = decode(&registration.public_key)?;
    let client_data = decode(&registration.client_data_json)?;
    let authenticator_data = decode(&registration.authenticator_data)?;
    if credential_id.is_empty() || credential_id.len() > 1023 {
        return Err(invalid());
    }
    let (rp_id, origin) = relying_party(&state.public_url);
    if authenticator_data.len() < 37
        || authenticator_data[..32] != Sha256::digest(rp_id.as_bytes())[..]
        || authenticator_data[32] & 0x01 == 0
    {
        return Err(invalid());
    }
    #[derive(Deserialize)]
    struct Challenge {
        challenge: String,
    }
    let challenge = serde_json::from_slice::<Challenge>(&client_data)
        .ok()
        .and_then(|data| sewa_approval::decode(&data.challenge).ok())
        .ok_or_else(invalid)?;
    let mut transaction = state.pool.begin().await?;
    let known: bool = sqlx::query_scalar(
        "WITH used AS (
             DELETE FROM passkey_challenges WHERE challenge = $1 AND login = $2 RETURNING created_at
         )
         SELECT EXISTS (SELECT 1 FROM used WHERE created_at >= now() - make_interval(secs => $3))",
    )
    .bind(&challenge)
    .bind(&identity.login)
    .bind(CHALLENGE_LIFETIME as f64)
    .fetch_one(&mut *transaction)
    .await?;
    if !known {
        transaction.commit().await?;
        return Err(Failure::new(StatusCode::BAD_REQUEST, "unknown_challenge"));
    }
    sewa_approval::check_client_data(&client_data, "webauthn.create", &challenge, &origin)
        .map_err(|rejection| match rejection {
            Rejection::WrongOrigin => Failure::new(StatusCode::BAD_REQUEST, "wrong_origin"),
            _ => invalid(),
        })?;
    let row: Option<PasskeyRow> = sqlx::query_as(
        "INSERT INTO passkeys (login, name, credential_id, public_key) VALUES ($1, $2, $3, $4)
         ON CONFLICT (credential_id) DO NOTHING
         RETURNING id, login, name, credential_id, created_at",
    )
    .bind(&identity.login)
    .bind(name)
    .bind(&credential_id)
    .bind(&public_key)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(row) = row else {
        return Err(Failure::new(StatusCode::CONFLICT, "passkey_exists"));
    };
    let created = passkey(row);
    events::record_about(
        &mut transaction,
        About::default(),
        events::PASSKEY_REGISTERED,
        serde_json::json!({
            "by": identity.login,
            "passkey_id": created.id,
            "name": created.name,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[cfg(test)]
mod tests {
    use super::relying_party;

    #[test]
    fn the_relying_party_is_the_public_host() {
        assert_eq!(
            relying_party("https://sewa.example.com:8443"),
            (
                "sewa.example.com".to_owned(),
                "https://sewa.example.com:8443".to_owned()
            )
        );
        assert_eq!(
            relying_party("http://localhost:8080"),
            ("localhost".to_owned(), "http://localhost:8080".to_owned())
        );
    }
}
