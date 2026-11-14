//! The OpAMP server agents report to, at `POST /v1/opamp`.
//!
//! Plain HTTP transport: the agent posts a protobuf `AgentToServer` and gets
//! a protobuf `ServerToAgent` back. The controller keeps the agent's
//! description and when it last heard from it. Adapted from pg-yard's
//! `crates/cell/src/opamp.rs`, without remote configuration, health, or
//! custom messages.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use prost::Message;
use uuid::Uuid;

use crate::enroll::{authenticate, bearer};
use crate::error::ApiResult;
use crate::{AppState, events};
use proto::any_value::Value;
use proto::{
    AgentDescription, AgentToServer, ServerCapabilities, ServerToAgent, ServerToAgentFlags,
};

/// The messages of the OpAMP specification, generated from `proto/opamp`.
#[allow(clippy::all, clippy::pedantic)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/opamp.proto.v1.rs"));
}

/// What the controller keeps of an agent description.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Description {
    pub version: Option<String>,
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub user: Option<String>,
}

impl From<&AgentDescription> for Description {
    fn from(description: &AgentDescription) -> Self {
        let text = |key: &str| {
            description
                .identifying_attributes
                .iter()
                .chain(&description.non_identifying_attributes)
                .find(|attribute| attribute.key == key)
                .and_then(
                    |attribute| match attribute.value.as_ref()?.value.as_ref()? {
                        Value::StringValue(value) if !value.is_empty() => {
                            Some(value.replace('\0', ""))
                        }
                        _ => None,
                    },
                )
        };
        Self {
            version: text("service.version"),
            hostname: text("host.name"),
            os: text("os.type"),
            arch: text("host.arch"),
            user: text("process.owner"),
        }
    }
}

/// Takes one `AgentToServer`. A missing, unknown, or revoked credential gets
/// 401 and a body that is not an `AgentToServer` gets 400, both without a
/// body.
pub async fn exchange(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let Some(credential) = bearer(&headers) else {
        return Ok(StatusCode::UNAUTHORIZED.into_response());
    };
    let Some(host) = authenticate(&state.pool, credential).await? else {
        return Ok(StatusCode::UNAUTHORIZED.into_response());
    };
    let Ok(message) = AgentToServer::decode(body) else {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    };
    let Ok(instance) = Uuid::from_slice(&message.instance_uid) else {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    };
    let full_state = save(&state.pool, host, instance, &message).await?;
    let answer = ServerToAgent {
        instance_uid: message.instance_uid,
        capabilities: ServerCapabilities::AcceptsStatus as u64,
        flags: if full_state {
            ServerToAgentFlags::ReportFullState as u64
        } else {
            0
        },
        ..Default::default()
    };
    Ok((
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        answer.encode_to_vec(),
    )
        .into_response())
}

/// Records one message. Returns whether the controller needs the agent's
/// full state: a message without a description from an agent instance it
/// does not know.
async fn save(
    pool: &sqlx::PgPool,
    host: Uuid,
    instance: Uuid,
    message: &AgentToServer,
) -> sqlx::Result<bool> {
    let description = message.agent_description.as_ref().map(Description::from);
    let mut transaction = pool.begin().await?;
    let (was_connected, known): (bool, Option<Uuid>) =
        sqlx::query_as("SELECT connected, agent_instance_uid FROM hosts WHERE id = $1 FOR UPDATE")
            .bind(host)
            .fetch_one(&mut *transaction)
            .await?;
    let same_instance = known == Some(instance);
    // A message leaves out what did not change, so each value stays until a
    // description carries a new one, unless another agent instance speaks.
    sqlx::query(
        "UPDATE hosts SET
             last_seen_at = now(),
             connected = true,
             agent_instance_uid = $2,
             agent_version = CASE WHEN $3 THEN $4 WHEN $8 THEN agent_version END,
             agent_hostname = CASE WHEN $3 THEN $5 WHEN $8 THEN agent_hostname END,
             agent_os = CASE WHEN $3 THEN $6 WHEN $8 THEN agent_os END,
             agent_arch = CASE WHEN $3 THEN $7 WHEN $8 THEN agent_arch END,
             agent_user = CASE WHEN $3 THEN $9 WHEN $8 THEN agent_user END
         WHERE id = $1",
    )
    .bind(host)
    .bind(instance)
    .bind(description.is_some())
    .bind(description.as_ref().and_then(|d| d.version.as_deref()))
    .bind(description.as_ref().and_then(|d| d.hostname.as_deref()))
    .bind(description.as_ref().and_then(|d| d.os.as_deref()))
    .bind(description.as_ref().and_then(|d| d.arch.as_deref()))
    .bind(same_instance)
    .bind(description.as_ref().and_then(|d| d.user.as_deref()))
    .execute(&mut *transaction)
    .await?;
    if !was_connected {
        events::record(
            &mut transaction,
            host,
            events::HOST_CONNECTED,
            serde_json::json!({ "instance_uid": instance }),
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(description.is_none() && !same_instance)
}

#[cfg(test)]
mod tests {
    use super::proto::{AnyValue, KeyValue};
    use super::*;

    fn attribute(key: &str, value: &str) -> KeyValue {
        KeyValue {
            key: key.to_owned(),
            value: Some(AnyValue {
                value: Some(Value::StringValue(value.to_owned())),
            }),
        }
    }

    #[test]
    fn reads_the_description() {
        let description = AgentDescription {
            identifying_attributes: vec![
                attribute("service.name", "sewa-agent"),
                attribute("service.version", "0.1.0"),
            ],
            non_identifying_attributes: vec![
                attribute("host.name", "node-1"),
                attribute("os.type", "linux"),
                attribute("host.arch", "amd64"),
                attribute("process.owner", "root"),
            ],
        };
        assert_eq!(
            Description::from(&description),
            Description {
                version: Some("0.1.0".to_owned()),
                hostname: Some("node-1".to_owned()),
                os: Some("linux".to_owned()),
                arch: Some("amd64".to_owned()),
                user: Some("root".to_owned()),
            }
        );
    }
}
