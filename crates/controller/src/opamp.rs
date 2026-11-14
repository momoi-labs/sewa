//! The OpAMP server agents report to, at `POST /v1/opamp`.
//!
//! Plain HTTP transport: the agent posts a protobuf `AgentToServer` and gets
//! a protobuf `ServerToAgent` back. The controller keeps the agent's
//! description and when it last heard from it. Adapted from pg-yard's
//! `crates/cell/src/opamp.rs`.
//!
//! The controller sends each host its mise bootstrap configs as
//! `remote_config`, and the agent sends each plan back as a custom message
//! of the `io.sewa.plan` capability. Approvals go to the agent, and what
//! it did with them comes back, as custom messages of the
//! `io.sewa.approval` capability. See docs/api.md.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use prost::Message;
use uuid::Uuid;

use crate::enroll::{authenticate, bearer};
use crate::error::ApiResult;
use crate::{AppState, approvals, desired, events};
use proto::any_value::Value;
use proto::{
    AgentConfigMap, AgentConfigObject, AgentDescription, AgentRemoteConfig, AgentToServer,
    CustomCapabilities, CustomMessage, ServerCapabilities, ServerToAgent, ServerToAgentFlags,
};

/// The custom capability that carries plans from the agent.
pub const PLAN_CAPABILITY: &str = "io.sewa.plan";

/// The custom message type of one plan.
pub const PLAN_TYPE: &str = "plan";

/// The content type of a config in `remote_config`.
pub const CONFIG_CONTENT_TYPE: &str = "application/toml";

/// One plan, as the agent sends it in a custom message.
#[derive(Debug, serde::Deserialize)]
pub struct PlanReport {
    /// `current` or `change/<id>`.
    pub config: String,
    /// The hex SHA-256 of the config the agent planned.
    pub config_hash: String,
    /// The hex SHA-256 of mise's JSON output.
    pub plan_hash: String,
    /// mise's exit code with `--detailed-exitcode`: 0 in sync, 2 changes,
    /// 1 an error or an unknown state.
    pub exit_code: i32,
    /// mise's JSON plan, when it printed one.
    pub plan: Option<serde_json::Value>,
    pub error: Option<String>,
    /// With a plan of `current`: the packages the host's package manager
    /// lists as explicitly installed.
    pub explicit: Option<Explicit>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Explicit {
    /// `pacman`, `apt`, or `brew`.
    pub manager: String,
    pub packages: Vec<String>,
}

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
    if let Some(custom) = &message.custom_message
        && custom.capability == PLAN_CAPABILITY
        && custom.r#type == PLAN_TYPE
    {
        match serde_json::from_slice::<PlanReport>(&custom.data) {
            Ok(report) => save_plan(&state.pool, host, &report).await?,
            Err(error) => tracing::warn!(host = %host, %error, "cannot read a plan"),
        }
    }
    if let Some(custom) = &message.custom_message
        && custom.capability == sewa_approval::CAPABILITY
        && custom.r#type == sewa_approval::RESULT_TYPE
    {
        match serde_json::from_slice::<sewa_approval::Outcome>(&custom.data) {
            Ok(outcome) => approvals::save_outcome(&state.pool, host, &outcome).await?,
            Err(error) => tracing::warn!(host = %host, %error, "cannot read an apply result"),
        }
    }
    // The oldest approval the host has not reported on, on every answer
    // until it does.
    let approval = approvals::pending(&state.pool, host).await?;
    let remote_config = remote_config(&state.pool, host).await?;
    let known_hash = message
        .remote_config_status
        .as_ref()
        .map(|status| status.last_remote_config_hash.as_slice());
    let answer = ServerToAgent {
        instance_uid: message.instance_uid,
        capabilities: ServerCapabilities::AcceptsStatus as u64
            | ServerCapabilities::OffersRemoteConfig as u64,
        flags: if full_state {
            ServerToAgentFlags::ReportFullState as u64
        } else {
            0
        },
        // Sent only when the agent does not have it yet.
        remote_config: (known_hash != Some(remote_config.config_hash.as_slice()))
            .then_some(remote_config),
        custom_capabilities: Some(CustomCapabilities {
            capabilities: vec![
                PLAN_CAPABILITY.to_owned(),
                sewa_approval::CAPABILITY.to_owned(),
            ],
        }),
        custom_message: approval.map(|approval| CustomMessage {
            capability: sewa_approval::CAPABILITY.to_owned(),
            r#type: sewa_approval::APPROVAL_TYPE.to_owned(),
            data: serde_json::to_vec(&approval).unwrap_or_default(),
        }),
        ..Default::default()
    };
    Ok((
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        answer.encode_to_vec(),
    )
        .into_response())
}

/// The configs of a host, as OpAMP's `remote_config`.
async fn remote_config(pool: &sqlx::PgPool, host: Uuid) -> sqlx::Result<AgentRemoteConfig> {
    let mut connection = pool.acquire().await?;
    let (approved, proposed) = desired::states(&mut connection).await?;
    let configs = desired::host_configs(host, &approved, proposed.as_ref());
    Ok(AgentRemoteConfig {
        config_hash: desired::configs_hash(&configs),
        config: Some(AgentConfigMap {
            config_map: configs
                .into_iter()
                .map(|(name, body)| {
                    (
                        name,
                        AgentConfigObject {
                            body: body.into_bytes(),
                            content_type: CONFIG_CONTENT_TYPE.to_owned(),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
        }),
    })
}

/// Keeps the last plan of each config. A plan of `current` also updates
/// the host's explicit packages, and records `drift.detected` when it shows
/// a drift other than the last one recorded.
async fn save_plan(pool: &sqlx::PgPool, host: Uuid, report: &PlanReport) -> sqlx::Result<()> {
    let change = if report.config == desired::CURRENT {
        None
    } else {
        match report
            .config
            .strip_prefix("change/")
            .and_then(|id| id.parse::<Uuid>().ok())
        {
            Some(change) => Some(change),
            None => {
                tracing::warn!(host = %host, config = report.config, "a plan of an unknown config");
                return Ok(());
            }
        }
    };
    let mut transaction = pool.begin().await?;
    if let Some(change) = change {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM changes WHERE id = $1)")
                .bind(change)
                .fetch_one(&mut *transaction)
                .await?;
        if !exists {
            return Ok(());
        }
    }
    let text = |value: &str| value.replace('\0', "");
    sqlx::query(
        "INSERT INTO host_plans
             (host_id, config, change_id, config_hash, plan_hash, exit_code, plan, error)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (host_id, config) DO UPDATE SET
             change_id = $3, config_hash = $4, plan_hash = $5, exit_code = $6,
             plan = $7, error = $8, reported_at = now()",
    )
    .bind(host)
    .bind(&report.config)
    .bind(change)
    .bind(text(&report.config_hash))
    .bind(text(&report.plan_hash))
    .bind(report.exit_code)
    .bind(&report.plan)
    .bind(report.error.as_deref().map(text))
    .execute(&mut *transaction)
    .await?;
    if change.is_none() {
        if let Some(explicit) = &report.explicit {
            let packages: Vec<String> = explicit.packages.iter().map(|p| text(p)).collect();
            sqlx::query(
                "UPDATE hosts SET explicit_manager = $2, explicit_packages = $3,
                     explicit_reported_at = now()
                 WHERE id = $1",
            )
            .bind(host)
            .bind(text(&explicit.manager))
            .bind(packages)
            .execute(&mut *transaction)
            .await?;
        }
        match report.exit_code {
            2 => {
                let last: Option<String> = sqlx::query_scalar(
                    "SELECT drift_plan_hash FROM hosts WHERE id = $1 FOR UPDATE",
                )
                .bind(host)
                .fetch_one(&mut *transaction)
                .await?;
                if last.as_deref() != Some(report.plan_hash.as_str()) {
                    sqlx::query("UPDATE hosts SET drift_plan_hash = $2 WHERE id = $1")
                        .bind(host)
                        .bind(text(&report.plan_hash))
                        .execute(&mut *transaction)
                        .await?;
                    let summary = report
                        .plan
                        .as_ref()
                        .and_then(|plan| plan.get("summary").cloned())
                        .unwrap_or_default();
                    events::record(
                        &mut transaction,
                        host,
                        events::DRIFT_DETECTED,
                        serde_json::json!({
                            "config_hash": report.config_hash,
                            "plan_hash": report.plan_hash,
                            "summary": summary,
                        }),
                    )
                    .await?;
                }
            }
            0 => {
                sqlx::query("UPDATE hosts SET drift_plan_hash = NULL WHERE id = $1")
                    .bind(host)
                    .execute(&mut *transaction)
                    .await?;
            }
            // An error or an unknown state says nothing about drift.
            _ => {}
        }
    }
    transaction.commit().await
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
             agent_user = CASE WHEN $3 THEN $9 WHEN $8 THEN agent_user END,
             health = COALESCE($10, health)
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
    .bind(
        message
            .health
            .as_ref()
            .map(|health| health.status.replace('\0', ""))
            .filter(|status| !status.is_empty()),
    )
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
