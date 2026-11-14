//! Who is behind a connection, as Tailscale knows it.
//!
//! The controller asks tailscaled's local API (`/localapi/v0/whois`) for the
//! peer address of each request. The answer names the tailnet node and the
//! user that owns it. An address that is not a tailnet node has no answer.
//! [`WhoIs`] hides the lookup, so tests use a fake.

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, bail};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// A tailnet node and the user that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The user's login name, as the admin list holds it.
    pub login: String,
    /// The node's name in the tailnet.
    pub node: String,
}

pub type WhoIsFuture<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<Option<Identity>>> + Send + 'a>>;

/// Finds who is behind a peer address. `Ok(None)` means the address is not
/// a tailnet node; an error means the lookup itself failed.
pub trait WhoIs: Send + Sync {
    fn whois(&self, peer: SocketAddr) -> WhoIsFuture<'_>;
}

/// The local API of the tailscaled on this machine, over its Unix socket.
pub struct Tailscale {
    pub socket: PathBuf,
}

/// The largest answer read from tailscaled.
const ANSWER_LIMIT: u64 = 1024 * 1024;

const DEADLINE: Duration = Duration::from_secs(5);

impl WhoIs for Tailscale {
    fn whois(&self, peer: SocketAddr) -> WhoIsFuture<'_> {
        Box::pin(async move {
            tokio::time::timeout(DEADLINE, self.ask(peer))
                .await
                .context("tailscaled did not answer in time")?
        })
    }
}

impl Tailscale {
    async fn ask(&self, peer: SocketAddr) -> anyhow::Result<Option<Identity>> {
        let mut stream = UnixStream::connect(&self.socket)
            .await
            .with_context(|| format!("cannot connect to {}", self.socket.display()))?;
        // HTTP/1.0, so the answer is never chunked and ends when the
        // connection closes.
        let request = format!(
            "GET /localapi/v0/whois?addr={} HTTP/1.0\r\nHost: local-tailscaled.sock\r\n\r\n",
            query_escape(&peer.to_string())
        );
        stream.write_all(request.as_bytes()).await?;
        let mut answer = Vec::new();
        stream.take(ANSWER_LIMIT).read_to_end(&mut answer).await?;
        parse_answer(&answer)
    }
}

/// Percent-encodes everything but letters, digits, and dots.
fn query_escape(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || byte == b'.' {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[derive(Deserialize)]
struct Answer {
    #[serde(rename = "Node")]
    node: Node,
    #[serde(rename = "UserProfile")]
    user: UserProfile,
}

#[derive(Deserialize)]
struct Node {
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Deserialize)]
struct UserProfile {
    #[serde(rename = "LoginName")]
    login: String,
}

/// Reads tailscaled's HTTP answer: 200 with the node, or 404 when the
/// address is not a tailnet node.
fn parse_answer(answer: &[u8]) -> anyhow::Result<Option<Identity>> {
    let split = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .context("tailscaled sent no HTTP header")?;
    let head = std::str::from_utf8(&answer[..split]).context("tailscaled sent a bad header")?;
    let body = &answer[split + 4..];
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .context("tailscaled sent no status")?;
    match status {
        "200" => {
            let answer: Answer =
                serde_json::from_slice(body).context("tailscaled sent an unknown answer")?;
            Ok(Some(Identity {
                login: answer.user.login,
                node: answer.node.name.trim_end_matches('.').to_owned(),
            }))
        }
        "404" => Ok(None),
        other => bail!("tailscaled answered {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_the_address() {
        assert_eq!(query_escape("192.0.2.1:41641"), "192.0.2.1%3A41641");
        assert_eq!(
            query_escape("[2001:db8::1]:80"),
            "%5B2001%3Adb8%3A%3A1%5D%3A80"
        );
    }

    #[test]
    fn reads_a_node() {
        let answer = b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n\
            {\"Node\":{\"Name\":\"laptop.example.\",\"ID\":1},\
             \"UserProfile\":{\"LoginName\":\"admin@example.com\"},\"CapMap\":{}}";
        assert_eq!(
            parse_answer(answer).unwrap(),
            Some(Identity {
                login: "admin@example.com".to_owned(),
                node: "laptop.example".to_owned(),
            })
        );
    }

    #[test]
    fn an_unknown_address_is_no_node() {
        let answer = b"HTTP/1.0 404 Not Found\r\n\r\nno match for IP:port\n";
        assert_eq!(parse_answer(answer).unwrap(), None);
    }

    #[test]
    fn another_status_is_an_error() {
        let answer = b"HTTP/1.0 403 Forbidden\r\n\r\ndenied\n";
        assert!(parse_answer(answer).is_err());
    }
}
