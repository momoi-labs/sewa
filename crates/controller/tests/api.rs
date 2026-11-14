//! The controller's HTTP API against a real PostgreSQL, with a fake WhoIs.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use prost::Message;
use serde_json::{Value, json};
use sewa_controller::install::AgentFiles;
use sewa_controller::opamp::proto::{
    AgentDescription, AgentToServer, AnyValue, KeyValue, ServerToAgent, ServerToAgentFlags,
    any_value,
};
use sewa_controller::whois::{Identity, WhoIs, WhoIsFuture};
use sewa_controller::{AppState, router, status};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// The admin's laptop, another user's node, and an address off the tailnet.
const ADMIN: &str = "192.0.2.1:50000";
const OTHER_USER: &str = "192.0.2.2:50000";
const NODE: &str = "192.0.2.3:50000";
const OFF_TAILNET: &str = "198.51.100.1:50000";

struct FakeWhoIs(HashMap<std::net::IpAddr, Identity>);

impl WhoIs for FakeWhoIs {
    fn whois(&self, peer: SocketAddr) -> WhoIsFuture<'_> {
        let identity = self.0.get(&peer.ip()).cloned();
        Box::pin(async move { Ok(identity) })
    }
}

fn identity(login: &str, node: &str) -> Identity {
    Identity {
        login: login.to_owned(),
        node: node.to_owned(),
    }
}

fn app(pool: PgPool) -> Router {
    let ip = |address: &str| address.parse::<SocketAddr>().unwrap().ip();
    let whois = FakeWhoIs(HashMap::from([
        (ip(ADMIN), identity("admin@example.com", "laptop")),
        (ip(OTHER_USER), identity("guest@example.com", "phone")),
        (ip(NODE), identity("admin@example.com", "node-1")),
    ]));
    router(AppState {
        pool,
        whois: Arc::new(whois),
        admins: Arc::new(vec!["admin@example.com".to_owned()]),
        public_url: "http://controller.example:8080".to_owned(),
        agent_files: Arc::new(
            AgentFiles::default().with_binary("x86_64-unknown-linux-musl", b"agent"),
        ),
    })
}

async fn call(app: &Router, from: &str, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let mut request = request;
    request
        .extensions_mut()
        .insert(ConnectInfo(from.parse::<SocketAddr>().unwrap()));
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, body.to_vec())
}

async fn json_call(
    app: &Router,
    from: &str,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .unwrap();
    let (status, body) = call(app, from, request).await;
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

/// Creates a host and returns its id and install token.
async fn create_host(app: &Router, name: &str) -> (String, String) {
    let (status, created) = json_call(
        app,
        ADMIN,
        "POST",
        "/api/hosts",
        Some(json!({ "name": name, "os": "linux" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let command = created["install_command"].as_str().unwrap();
    let token = command
        .rsplit(' ')
        .next()
        .unwrap()
        .trim_matches('\'')
        .to_owned();
    (created["host"]["id"].as_str().unwrap().to_owned(), token)
}

async fn enroll(app: &Router, from: &str, token: &str) -> (StatusCode, String) {
    let request = Request::post("/v1/enroll")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "token": token }).to_string()))
        .unwrap();
    let (status, body) = call(app, from, request).await;
    (status, String::from_utf8(body).unwrap().trim().to_owned())
}

fn attribute(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value.to_owned())),
        }),
    }
}

fn report(instance: Uuid, described: bool) -> AgentToServer {
    AgentToServer {
        instance_uid: instance.as_bytes().to_vec(),
        agent_description: described.then(|| AgentDescription {
            identifying_attributes: vec![attribute("service.version", "0.1.0")],
            non_identifying_attributes: vec![
                attribute("host.name", "node-1"),
                attribute("os.type", "linux"),
                attribute("host.arch", "amd64"),
                attribute("process.owner", "root"),
            ],
        }),
        ..Default::default()
    }
}

async fn opamp(app: &Router, credential: &str, message: &AgentToServer) -> (StatusCode, Vec<u8>) {
    let request = Request::post("/v1/opamp")
        .header(header::CONTENT_TYPE, "application/x-protobuf")
        .header(header::AUTHORIZATION, format!("Bearer {credential}"))
        .body(Body::from(message.encode_to_vec()))
        .unwrap();
    call(app, NODE, request).await
}

async fn events(pool: &PgPool, host: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT kind FROM events WHERE host_id = $1::uuid ORDER BY id")
        .bind(host)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn only_admins_on_the_tailnet_use_the_api(pool: PgPool) {
    let app = app(pool);
    let (status, body) = json_call(&app, OTHER_USER, "GET", "/api/hosts", None).await;
    assert_eq!(
        (status, body),
        (StatusCode::FORBIDDEN, json!({ "error": "not_admin" }))
    );
    let (status, body) = json_call(&app, OFF_TAILNET, "GET", "/api/hosts", None).await;
    assert_eq!(
        (status, body),
        (StatusCode::FORBIDDEN, json!({ "error": "not_tailnet" }))
    );
    let (status, body) = json_call(&app, ADMIN, "GET", "/api/hosts", None).await;
    assert_eq!((status, body), (StatusCode::OK, json!([])));
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn a_new_host_waits_with_an_install_command(pool: PgPool) {
    let app = app(pool.clone());
    let (status, created) = json_call(
        &app,
        ADMIN,
        "POST",
        "/api/hosts",
        Some(json!({ "name": "node-1", "os": "macos" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["host"]["status"], "waiting");
    assert!(
        created["install_command"]
            .as_str()
            .unwrap()
            .starts_with("curl -fsSL 'http://controller.example:8080/install' | sudo sh -s -- '")
    );
    let (_, hosts) = json_call(&app, ADMIN, "GET", "/api/hosts", None).await;
    assert_eq!(hosts[0]["name"], "node-1");
    assert_eq!(hosts[0]["os"], "macos");
    assert_eq!(hosts[0]["status"], "waiting");
    assert_eq!(hosts[0]["agent"], Value::Null);
    let id = created["host"]["id"].as_str().unwrap();
    assert_eq!(events(&pool, id).await, ["host.created"]);

    let (status, body) = json_call(
        &app,
        ADMIN,
        "POST",
        "/api/hosts",
        Some(json!({ "name": "node-1", "os": "linux" })),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "name_taken" }))
    );
    let (status, body) = json_call(
        &app,
        ADMIN,
        "POST",
        "/api/hosts",
        Some(json!({ "name": "node-2", "os": "windows" })),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!({ "error": "invalid_os" }))
    );
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn a_token_enrolls_once_from_the_tailnet(pool: PgPool) {
    let app = app(pool.clone());
    let (id, token) = create_host(&app, "node-1").await;
    assert_eq!(
        enroll(&app, OFF_TAILNET, &token).await.0,
        StatusCode::FORBIDDEN
    );
    let (status, credential) = enroll(&app, NODE, &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!credential.is_empty());
    assert_eq!(enroll(&app, NODE, &token).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        enroll(&app, NODE, "unknown").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(events(&pool, &id).await, ["host.created", "host.enrolled"]);
    // Only hashes are stored.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT credential_hash FROM agent_credentials")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(stored, credential.as_bytes());
    let (_, hosts) = json_call(&app, ADMIN, "GET", "/api/hosts", None).await;
    assert_eq!(hosts[0]["status"], "disconnected");
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn an_expired_token_does_not_enroll(pool: PgPool) {
    let app = app(pool.clone());
    let (_, token) = create_host(&app, "node-1").await;
    sqlx::query("UPDATE install_tokens SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(enroll(&app, NODE, &token).await.0, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn reports_connect_the_host_until_it_goes_quiet(pool: PgPool) {
    let app = app(pool.clone());
    let (id, token) = create_host(&app, "node-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    let instance = Uuid::new_v4();

    assert_eq!(
        opamp(&app, "wrong", &report(instance, true)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, body) = opamp(&app, &credential, &report(instance, true)).await;
    assert_eq!(status, StatusCode::OK);
    let answer = ServerToAgent::decode(&body[..]).unwrap();
    assert_eq!(answer.instance_uid, instance.as_bytes().to_vec());
    assert_eq!(answer.flags, 0);
    // A second report records no second host.connected.
    assert_eq!(
        opamp(&app, &credential, &report(instance, false)).await.0,
        StatusCode::OK
    );

    let (_, hosts) = json_call(&app, ADMIN, "GET", "/api/hosts", None).await;
    assert_eq!(hosts[0]["status"], "connected");
    assert_eq!(
        hosts[0]["agent"],
        json!({ "version": "0.1.0", "hostname": "node-1", "os": "linux", "arch": "amd64", "user": "root" })
    );

    // Another agent instance without a description is asked for its full state.
    let (_, body) = opamp(&app, &credential, &report(Uuid::new_v4(), false)).await;
    assert_eq!(
        ServerToAgent::decode(&body[..]).unwrap().flags,
        ServerToAgentFlags::ReportFullState as u64
    );

    assert_eq!(status::sweep(&pool).await.unwrap(), 0);
    sqlx::query("UPDATE hosts SET last_seen_at = now() - interval '91 seconds'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(status::sweep(&pool).await.unwrap(), 1);
    assert_eq!(status::sweep(&pool).await.unwrap(), 0);
    let (_, hosts) = json_call(&app, ADMIN, "GET", "/api/hosts", None).await;
    assert_eq!(hosts[0]["status"], "disconnected");

    assert_eq!(
        opamp(&app, &credential, &report(instance, true)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        events(&pool, &id).await,
        [
            "host.created",
            "host.enrolled",
            "host.connected",
            "host.disconnected",
            "host.connected"
        ]
    );
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn a_revoked_credential_is_refused(pool: PgPool) {
    let app = app(pool.clone());
    let (_, token) = create_host(&app, "node-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    sqlx::query("UPDATE agent_credentials SET revoked_at = now()")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        opamp(&app, &credential, &report(Uuid::new_v4(), true))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn events_are_append_only(pool: PgPool) {
    let app = app(pool.clone());
    create_host(&app, "node-1").await;
    assert!(
        sqlx::query("DELETE FROM events")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE events SET kind = 'host.enrolled'")
            .execute(&pool)
            .await
            .is_err()
    );
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn serves_the_script_and_the_binaries(pool: PgPool) {
    let app = app(pool);
    let (status, script) = call(
        &app,
        OFF_TAILNET,
        Request::get("/install").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let script = String::from_utf8(script).unwrap();
    assert!(script.contains("x86_64-unknown-linux-musl) sum='"));
    let (status, binary) = call(
        &app,
        OFF_TAILNET,
        Request::get("/files/sewa-agent-x86_64-unknown-linux-musl")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        (status, binary.as_slice()),
        (StatusCode::OK, b"agent".as_slice())
    );
    let (status, _) = call(
        &app,
        OFF_TAILNET,
        Request::get("/files/other").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn a_host_reads_with_its_events_newest_first(pool: PgPool) {
    let app = app(pool);
    let (id, token) = create_host(&app, "node-1").await;
    enroll(&app, NODE, &token).await;

    let (status, host) = json_call(&app, ADMIN, "GET", &format!("/api/hosts/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(host["name"], "node-1");
    assert_eq!(host["status"], "disconnected");

    let uri = format!("/api/hosts/{id}/events");
    let (status, events) = json_call(&app, ADMIN, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    let kinds: Vec<&str> = events
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["host.enrolled", "host.created"]);
    assert_eq!(
        events[1]["data"],
        json!({ "name": "node-1", "os": "linux" })
    );
    assert!(events[1]["at"].is_string());

    let (status, _) = json_call(&app, OTHER_USER, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for uri in [
        format!("/api/hosts/{}", Uuid::new_v4()),
        format!("/api/hosts/{}/events", Uuid::new_v4()),
        "/api/hosts/not-an-id".to_owned(),
        "/api/unknown".to_owned(),
    ] {
        let (status, body) = json_call(&app, ADMIN, "GET", &uri, None).await;
        assert_eq!(
            (status, body),
            (StatusCode::NOT_FOUND, json!({ "error": "not_found" })),
            "{uri}"
        );
    }
}
