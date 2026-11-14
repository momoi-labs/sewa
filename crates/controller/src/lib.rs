//! The sewa controller: hosts, their enrollment, and what their agents
//! report over OpAMP.

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRef};
use axum::middleware;
use axum::routing::{get, post};
use sqlx::PgPool;
use sqlx::migrate::Migrator;

use crate::install::AgentFiles;
use crate::whois::WhoIs;

pub mod enroll;
pub mod error;
pub mod events;
pub mod hosts;
pub mod install;
pub mod opamp;
pub mod secret;
pub mod status;
pub mod whois;

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub whois: Arc<dyn WhoIs>,
    /// The Tailscale login names that may call the API.
    pub admins: Arc<Vec<String>>,
    /// The address agents reach the controller at, without a trailing
    /// slash.
    pub public_url: String,
    pub agent_files: Arc<AgentFiles>,
}

impl FromRef<AppState> for Arc<AgentFiles> {
    fn from_ref(state: &AppState) -> Self {
        state.agent_files.clone()
    }
}

/// The largest OpAMP message the controller reads.
const OPAMP_LIMIT: usize = 1024 * 1024;

pub fn router(state: AppState) -> Router {
    // Rendered once: the script holds the controller's constants and no
    // secret.
    let script = Bytes::from(install::script(
        &state.public_url,
        &state.agent_files.sums(),
    ));
    let api = Router::new()
        .route("/hosts", get(hosts::list).post(hosts::create))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            hosts::require_admin,
        ));
    Router::new()
        .nest("/api", api)
        .route("/v1/enroll", post(enroll::enroll))
        .route(
            "/v1/opamp",
            post(opamp::exchange).layer(DefaultBodyLimit::max(OPAMP_LIMIT)),
        )
        .route("/files/{name}", get(install::serve_file))
        .route("/install", get(install::serve_script).with_state(script))
        .with_state(state)
}
