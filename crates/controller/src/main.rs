use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use sewa_controller::install::AgentFiles;
use sewa_controller::whois::Tailscale;
use sewa_controller::{AppState, MIGRATOR, router, status};

/// The sewa controller: hosts, their enrollment, and their status.
#[derive(Parser)]
#[command(name = "sewa-controller", version)]
struct Args {
    /// PostgreSQL connection URL.
    #[arg(long, env = "DATABASE_URL", hide_env_values = true)]
    database_url: String,

    /// Address and port to listen on, such as this machine's Tailscale
    /// address.
    #[arg(long, env = "SEWA_LISTEN")]
    listen: SocketAddr,

    /// The address agents reach the controller at, such as
    /// http://controller.example:8080. It goes into the install command.
    #[arg(long, env = "SEWA_PUBLIC_URL", value_parser = parse_public_url)]
    public_url: String,

    /// The Tailscale login names that may call the API, comma-separated.
    #[arg(long, env = "SEWA_ADMINS", value_delimiter = ',', required = true)]
    admins: Vec<String>,

    /// The directory with the agent binary of each target, named
    /// sewa-agent-<target>. dev/agent-files.sh builds them.
    #[arg(long, env = "SEWA_AGENT_DIR")]
    agent_dir: PathBuf,

    /// tailscaled's local API socket.
    #[arg(
        long,
        env = "SEWA_TAILSCALE_SOCKET",
        default_value = "/var/run/tailscale/tailscaled.sock"
    )]
    tailscale_socket: PathBuf,
}

/// An http or https URL without credentials, query, or fragment, with the
/// trailing slash removed.
fn parse_public_url(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/');
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .ok_or("must start with http:// or https://")?;
    if rest.is_empty() || rest.contains(['@', '?', '#', '\'', ' ']) {
        return Err("must name a host, without credentials, query, or fragment".to_owned());
    }
    Ok(value.to_owned())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let admins: Vec<String> = args
        .admins
        .into_iter()
        .map(|admin| admin.trim().to_owned())
        .filter(|admin| !admin.is_empty())
        .collect();
    anyhow::ensure!(!admins.is_empty(), "SEWA_ADMINS names no one");
    let agent_files = AgentFiles::load(&args.agent_dir)?;
    let pool = sqlx::PgPool::connect(&args.database_url)
        .await
        .context("cannot connect to the database")?;
    MIGRATOR.run(&pool).await.context("cannot migrate")?;
    tokio::spawn(status::run_sweeper(pool.clone()));
    let state = AppState {
        pool,
        whois: Arc::new(Tailscale {
            socket: args.tailscale_socket,
        }),
        admins: Arc::new(admins),
        public_url: args.public_url,
        agent_files: Arc::new(agent_files),
    };
    let listener = tokio::net::TcpListener::bind(args.listen)
        .await
        .with_context(|| format!("cannot listen on {}", args.listen))?;
    tracing::info!(listen = %args.listen, "sewa-controller serves");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_public_url;

    #[test]
    fn public_urls() {
        assert_eq!(
            parse_public_url("http://controller.example:8080/").unwrap(),
            "http://controller.example:8080"
        );
        assert!(parse_public_url("controller.example").is_err());
        assert!(parse_public_url("http://user@controller.example").is_err());
        assert!(parse_public_url("https://").is_err());
    }
}
