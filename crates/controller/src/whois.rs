use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, bail};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub login: String,
    pub node: String,
}

pub type WhoIsFuture<'a> =
    Pin<Box<dyn Future<Output = anyhow::Result<Option<Identity>>> + Send + 'a>>;

pub trait WhoIs: Send + Sync {
    fn whois(&self, peer: SocketAddr) -> WhoIsFuture<'_>;
}

pub struct Tailscale {
    pub socket: PathBuf,
}

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
