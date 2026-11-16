use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, FromRef};
use axum::middleware;
use axum::routing::{get, post};
use sqlx::PgPool;
use sqlx::migrate::Migrator;

use crate::install::AgentFiles;
use crate::whois::WhoIs;

pub mod desired;
pub mod drift;
pub mod enroll;
pub mod error;
pub mod events;
pub mod hosts;
pub mod install;
pub mod opamp;
pub mod passkeys;
pub mod roles;
pub mod secret;
pub mod status;
pub mod syncs;
pub mod web;
pub mod whois;

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub whois: Arc<dyn WhoIs>,
    pub admins: Arc<Vec<String>>,
    pub public_url: String,
    pub agent_files: Arc<AgentFiles>,
}

impl FromRef<AppState> for Arc<AgentFiles> {
    fn from_ref(state: &AppState) -> Self {
        state.agent_files.clone()
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Instance {
    pub address: String,
}

async fn instance(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> axum::Json<Instance> {
    axum::Json(Instance {
        address: state.public_url.clone(),
    })
}

const OPAMP_LIMIT: usize = 1024 * 1024;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/hosts", get(hosts::list).post(hosts::create))
        .route("/hosts/{id}", get(hosts::get).delete(hosts::delete))
        .route("/hosts/{id}/disable", post(hosts::disable))
        .route("/hosts/{id}/enable", post(hosts::enable))
        .route("/hosts/{id}/events", get(hosts::events))
        .route("/hosts/{id}/state", get(hosts::state))
        .route("/hosts/{id}/edits", post(hosts::edit))
        .route("/hosts/{id}/drift/adopt", post(drift::adopt))
        .route("/hosts/{id}/drift/ignore", post(drift::ignore))
        .route("/passkeys", get(passkeys::list).post(passkeys::register))
        .route("/passkeys/options", post(passkeys::options))
        .route("/roles", get(roles::list).post(roles::create))
        .route("/roles/{id}", get(roles::get).put(roles::save))
        .route("/instance", get(instance))
        .route("/syncs", get(syncs::list).post(syncs::create))
        .route("/syncs/edits", get(syncs::edits))
        .route("/syncs/{id}", get(syncs::get))
        .route("/syncs/{id}/approval", post(syncs::options))
        .route("/syncs/{id}/approve", post(syncs::approve))
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
        .route("/install", get(install::serve_script))
        .fallback(web::serve)
        .with_state(state)
}
