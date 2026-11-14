//! The OpAMP client: the agent reports who it is to the controller over the
//! specification's plain HTTP transport, which polls instead of holding a
//! WebSocket. Adapted from pg-yard's `crates/agent/src/opamp.rs`, without
//! health, remote configuration, or custom messages.

use std::fmt;
use std::time::Duration;

use prost::Message;
use uuid::Uuid;

/// The messages of the OpAMP specification, generated from `proto/opamp`.
#[allow(clippy::all, clippy::pedantic)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/opamp.proto.v1.rs"));
}

use proto::{
    AgentCapabilities, AgentDescription, AgentToServer, AnyValue, KeyValue, ServerToAgent,
    any_value,
};

/// The version the agent reports.
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The `service.name` the agent reports.
pub const SERVICE_NAME: &str = "sewa-agent";

/// The specification's default polling interval.
pub const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// The largest response body the client reads.
const RESPONSE_LIMIT: u64 = 1024 * 1024;

/// Where the agent runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub hostname: String,
    /// `os.type` as the OpenTelemetry semantic conventions name it.
    pub os: &'static str,
    /// `host.arch` as the OpenTelemetry semantic conventions name it.
    pub arch: &'static str,
    /// The user the agent runs as.
    pub user: String,
}

impl Host {
    pub fn this() -> Self {
        let hostname = nix::unistd::gethostname()
            .ok()
            .and_then(|name| name.into_string().ok())
            .unwrap_or_else(|| "unknown".to_owned());
        let user = nix::unistd::User::from_uid(nix::unistd::getuid())
            .ok()
            .flatten()
            .map_or_else(|| nix::unistd::getuid().to_string(), |user| user.name);
        Self {
            hostname,
            os: match std::env::consts::OS {
                "macos" => "darwin",
                other => other,
            },
            arch: match std::env::consts::ARCH {
                "x86_64" => "amd64",
                "aarch64" => "arm64",
                other => other,
            },
            user,
        }
    }
}

/// The agent description: the identifying attributes name this agent, the
/// others describe where it runs.
pub fn describe(instance_uid: Uuid, host: &Host) -> AgentDescription {
    AgentDescription {
        identifying_attributes: vec![
            attribute("service.name", SERVICE_NAME),
            attribute("service.version", AGENT_VERSION),
            attribute("service.instance.id", &instance_uid.to_string()),
        ],
        non_identifying_attributes: vec![
            attribute("host.name", &host.hostname),
            attribute("os.type", host.os),
            attribute("host.arch", host.arch),
            attribute("process.owner", &host.user),
        ],
    }
}

fn attribute(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value.to_owned())),
        }),
    }
}

/// One status report. Each carries the full description, so a controller
/// that lost its copy needs nothing more.
pub fn message(instance_uid: Uuid, sequence_num: u64, host: &Host) -> AgentToServer {
    AgentToServer {
        instance_uid: instance_uid.as_bytes().to_vec(),
        sequence_num,
        agent_description: Some(describe(instance_uid, host)),
        capabilities: AgentCapabilities::ReportsStatus as u64,
        ..Default::default()
    }
}

/// Why an exchange failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeError {
    /// A network failure or a missed deadline.
    Network,
    Unauthorized,
    Status(u16),
    /// The response was too large or did not decode.
    Response,
}

impl fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network => f.write_str("network error or missed deadline"),
            Self::Unauthorized => {
                f.write_str("the controller refused the credential (401); enroll again")
            }
            Self::Status(status) => write!(f, "the controller answered {status}"),
            Self::Response => f.write_str("the response was too large or did not decode"),
        }
    }
}

/// One blocking HTTP client with short deadlines, no proxy, and no
/// redirects.
pub fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(10)))
        .max_redirects(0)
        .http_status_as_error(false)
        .proxy(None)
        .user_agent(format!("{SERVICE_NAME}/{AGENT_VERSION}"))
        .build()
        .into()
}

/// Sends one message to `url` and returns the answer.
pub fn exchange(
    agent: &ureq::Agent,
    url: &str,
    credential: &str,
    message: &AgentToServer,
) -> Result<ServerToAgent, ExchangeError> {
    let mut response = agent
        .post(url)
        .header("Content-Type", "application/x-protobuf")
        .header("Authorization", format!("Bearer {credential}"))
        .send(&message.encode_to_vec()[..])
        .map_err(|_| ExchangeError::Network)?;
    match response.status().as_u16() {
        200..=299 => {}
        401 => return Err(ExchangeError::Unauthorized),
        status => return Err(ExchangeError::Status(status)),
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(RESPONSE_LIMIT)
        .read_to_vec()
        .map_err(|_| ExchangeError::Response)?;
    ServerToAgent::decode(&body[..]).map_err(|_| ExchangeError::Response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text<'a>(description: &'a AgentDescription, key: &str) -> Option<&'a str> {
        description
            .identifying_attributes
            .iter()
            .chain(&description.non_identifying_attributes)
            .find(|attribute| attribute.key == key)
            .and_then(
                |attribute| match attribute.value.as_ref()?.value.as_ref()? {
                    any_value::Value::StringValue(value) => Some(value.as_str()),
                    _ => None,
                },
            )
    }

    #[test]
    fn the_message_describes_the_agent_and_its_host() {
        let uid = Uuid::now_v7();
        let host = Host {
            hostname: "node-1".to_owned(),
            os: "linux",
            arch: "amd64",
            user: "root".to_owned(),
        };
        let message = message(uid, 4, &host);
        assert_eq!(message.instance_uid, uid.as_bytes().to_vec());
        assert_eq!(message.sequence_num, 4);
        let description = message.agent_description.unwrap();
        assert_eq!(text(&description, "service.name"), Some("sewa-agent"));
        assert_eq!(text(&description, "service.version"), Some(AGENT_VERSION));
        assert_eq!(text(&description, "host.name"), Some("node-1"));
        assert_eq!(text(&description, "os.type"), Some("linux"));
        assert_eq!(text(&description, "host.arch"), Some("amd64"));
        assert_eq!(text(&description, "process.owner"), Some("root"));
    }

    #[test]
    fn agent_to_server_round_trips() {
        let host = Host::this();
        let message = message(Uuid::now_v7(), 3, &host);
        let bytes = message.encode_to_vec();
        assert_eq!(AgentToServer::decode(bytes.as_slice()).unwrap(), message);
    }
}
