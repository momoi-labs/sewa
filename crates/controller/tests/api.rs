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
use sewa_approval::testing::Authenticator;
use sewa_approval::{Outcome, Pinned, Signed};
use sewa_controller::install::AgentFiles;
use sewa_controller::opamp::proto::{
    AgentDescription, AgentToServer, AnyValue, CustomMessage, KeyValue, RemoteConfigStatus,
    ServerToAgent, ServerToAgentFlags, any_value,
};
use sewa_controller::whois::{Identity, WhoIs, WhoIsFuture};
use sewa_controller::{AppState, router, status};
use sha2::{Digest, Sha256};
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
        Some(json!({ "name": name })),
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
    report_os(instance, described, "linux")
}

/// A report whose description names `os` as `os.type`.
fn report_os(instance: Uuid, described: bool, os: &str) -> AgentToServer {
    AgentToServer {
        instance_uid: instance.as_bytes().to_vec(),
        agent_description: described.then(|| AgentDescription {
            identifying_attributes: vec![attribute("service.version", "0.1.0")],
            non_identifying_attributes: vec![
                attribute("host.name", "node-1"),
                attribute("os.type", os),
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
        Some(json!({ "name": "node-1" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["host"]["status"], "waiting");
    assert_eq!(created["host"]["os"], Value::Null);
    assert!(
        created["install_command"]
            .as_str()
            .unwrap()
            .starts_with("curl -fsSL 'http://controller.example:8080/install' | sudo sh -s -- '")
    );
    let (_, hosts) = json_call(&app, ADMIN, "GET", "/api/hosts", None).await;
    assert_eq!(hosts[0]["name"], "node-1");
    // The agent reports the OS when it connects.
    assert_eq!(hosts[0]["os"], Value::Null);
    assert_eq!(hosts[0]["status"], "waiting");
    assert_eq!(hosts[0]["agent"], Value::Null);
    let id = created["host"]["id"].as_str().unwrap();
    assert_eq!(events(&pool, id).await, ["host.created"]);

    let (status, body) = json_call(
        &app,
        ADMIN,
        "POST",
        "/api/hosts",
        Some(json!({ "name": "node-1" })),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "name_taken" }))
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
            "host.offline",
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
    assert_eq!(events[1]["data"], json!({ "name": "node-1" }));
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

async fn create_role(app: &Router, name: &str) -> String {
    let (status, role) = json_call(
        app,
        ADMIN,
        "POST",
        "/api/roles",
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{role}");
    role["id"].as_str().unwrap().to_owned()
}

async fn save_role(app: &Router, role: &str, body: Value) -> (StatusCode, Value) {
    json_call(app, ADMIN, "PUT", &format!("/api/roles/{role}"), Some(body)).await
}

async fn host_edits(app: &Router, host: &str, edits: Value) -> (StatusCode, Value) {
    json_call(
        app,
        ADMIN,
        "POST",
        &format!("/api/hosts/{host}/edits"),
        Some(json!({ "edits": edits })),
    )
    .await
}

async fn host_state(app: &Router, host: &str) -> Value {
    let (status, state) =
        json_call(app, ADMIN, "GET", &format!("/api/hosts/{host}/state"), None).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    state
}

fn plan_message(instance: Uuid, plan: Value) -> AgentToServer {
    AgentToServer {
        custom_message: Some(CustomMessage {
            capability: "io.sewa.plan".to_owned(),
            r#type: "plan".to_owned(),
            data: plan.to_string().into_bytes(),
        }),
        ..report(instance, false)
    }
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn roles_save_directly_and_record_what_changed(pool: PgPool) {
    let app = app(pool.clone());
    let role = create_role(&app, "base").await;
    let (status, body) = json_call(
        &app,
        ADMIN,
        "POST",
        "/api/roles",
        Some(json!({ "name": "base" })),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "name_taken" }))
    );

    let jq = json!({ "kind": "package", "name": "jq", "version": "latest", "os": null });
    let node = json!({ "kind": "tool", "name": "node", "version": "24", "os": "macos" });
    let (status, saved) =
        save_role(&app, &role, json!({ "name": "base", "items": [jq, node] })).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["items"], json!([jq, node]));

    for (body, error) in [
        (
            json!({ "name": "base", "items": [{ "kind": "tool", "name": "node", "version": "absent" }] }),
            "invalid_item",
        ),
        (
            json!({ "name": "base", "items": [{ "kind": "tool", "name": "node", "version": "1", "os": "windows" }] }),
            "invalid_item",
        ),
        (
            json!({ "name": "base", "items": [jq, jq] }),
            "duplicate_item",
        ),
        (json!({ "name": "two words", "items": [] }), "invalid_name"),
    ] {
        let (status, body) = save_role(&app, &role, body).await;
        assert_eq!(
            (status, body),
            (StatusCode::BAD_REQUEST, json!({ "error": error }))
        );
    }

    // A rename and an edit of the version: the event lists what changed.
    let (status, saved) = save_role(
        &app,
        &role,
        json!({ "name": "devtools", "items": [{ "kind": "tool", "name": "node", "version": "22", "os": "macos" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["name"], "devtools");
    let data: Vec<Value> =
        sqlx::query_scalar("SELECT data FROM events WHERE kind = 'role.saved' ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[1]["renamed_from"], "base");
    assert_eq!(data[1]["removed"][0]["name"], "jq");
    assert_eq!(data[1]["changed"][0]["version"], "22");
    assert_eq!(data[1]["added"], json!([]));

    // The edits since the last sync, which has not happened yet.
    let (_, edits) = json_call(&app, ADMIN, "GET", "/api/syncs/edits", None).await;
    assert_eq!(edits["since"], Value::Null);
    assert_eq!(edits["edits"].as_array().unwrap().len(), 2);
    assert_eq!(edits["edits"][1]["role"]["name"], "devtools");
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn the_bootstrap_role_is_built_in(pool: PgPool) {
    let app = app(pool.clone());
    let (_, roles) = json_call(&app, ADMIN, "GET", "/api/roles", None).await;
    let bootstrap = &roles[0];
    assert_eq!(bootstrap["name"], "bootstrap");
    assert_eq!(bootstrap["builtin"], true);
    assert_eq!(
        bootstrap["items"],
        json!([{ "kind": "package", "name": "mise", "version": "latest", "os": null }])
    );
    let id = bootstrap["id"].as_str().unwrap();
    // sewa manages it: every save is refused, even one that changes nothing,
    // and the items stay as they were.
    for saved in [
        json!({ "name": "base", "items": [] }),
        json!({ "name": "bootstrap", "items": [] }),
        json!({ "name": "bootstrap", "items": [
            { "kind": "package", "name": "mise", "version": "2026.1.0" }
        ] }),
        json!({ "name": "bootstrap", "items": [
            { "kind": "package", "name": "mise", "version": "latest" },
            { "kind": "package", "name": "git", "version": "latest" }
        ] }),
        json!({ "name": "bootstrap", "items": [
            { "kind": "package", "name": "mise", "version": "latest" }
        ] }),
    ] {
        let (status, body) = save_role(&app, id, saved).await;
        assert_eq!(
            (status, body),
            (StatusCode::CONFLICT, json!({ "error": "builtin_role" }))
        );
    }
    let (_, roles) = json_call(&app, ADMIN, "GET", "/api/roles", None).await;
    assert_eq!(roles[0]["items"], bootstrap["items"]);
    // Every host gets it with the OS's package manager.
    let (linux, token) = create_host(&app, "build-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    opamp(&app, &credential, &report(Uuid::new_v4(), true)).await;
    let (mac, token) = create_host(&app, "mac-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    opamp(
        &app,
        &credential,
        &report_os(Uuid::new_v4(), true, "darwin"),
    )
    .await;
    let mac = mac.as_str();
    let config = host_state(&app, &linux).await["config"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(config.contains("\"pacman:mise\" = \"latest\""), "{config}");
    let config = host_state(&app, mac).await["config"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(config.contains("\"brew:mise\" = \"latest\""), "{config}");

    // Adopting a package into it is refused too.
    let (status, body) = json_call(
        &app,
        ADMIN,
        "POST",
        &format!("/api/hosts/{linux}/drift/adopt"),
        Some(json!({ "package": "git", "role": id })),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "builtin_role" }))
    );

    // A host cannot drop or add the built-in role: it always has it.
    let (status, body) = host_edits(
        &app,
        &linux,
        json!([{ "op": "host_role_remove", "role": id }]),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!({ "error": "unknown_role" }))
    );
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn the_agent_reports_the_hosts_os(pool: PgPool) {
    let app = app(pool);
    let (host, token) = create_host(&app, "build-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    let uri = format!("/api/hosts/{host}");
    let os = |app: Router, uri: String| async move {
        json_call(&app, ADMIN, "GET", &uri, None).await.1["os"].clone()
    };
    assert_eq!(os(app.clone(), uri.clone()).await, Value::Null);

    // A Mac gets a Mac config from its first report.
    let instance = Uuid::new_v4();
    let (status, body) = opamp(&app, &credential, &report_os(instance, true, "darwin")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let remote = ServerToAgent::decode(&body[..])
        .unwrap()
        .remote_config
        .unwrap();
    let config =
        String::from_utf8_lossy(&remote.config.unwrap().config_map["current"].body).into_owned();
    assert!(config.contains("\"brew:mise\" = \"latest\""), "{config}");
    assert_eq!(os(app.clone(), uri.clone()).await, "macos");

    // Each later report sets it again; one without a description or with
    // an OS sewa does not know leaves it.
    opamp(&app, &credential, &report_os(instance, true, "linux")).await;
    assert_eq!(os(app.clone(), uri.clone()).await, "linux");
    opamp(&app, &credential, &report_os(instance, false, "darwin")).await;
    opamp(&app, &credential, &report_os(instance, true, "freebsd")).await;
    assert_eq!(os(app.clone(), uri.clone()).await, "linux");
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn hosts_get_roles_and_items_for_their_os(pool: PgPool) {
    let app = app(pool.clone());
    let (host, token) = create_host(&app, "build-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    opamp(&app, &credential, &report(Uuid::new_v4(), true)).await;
    let role = create_role(&app, "base").await;
    save_role(
        &app,
        &role,
        json!({ "name": "base", "items": [
            { "kind": "package", "name": "jq", "version": "latest" },
            { "kind": "package", "name": "brew-cask:firefox", "version": "latest", "os": "macos" },
            { "kind": "tool", "name": "node", "version": "24", "os": "linux" }
        ] }),
    )
    .await;
    let (status, state) = host_edits(
        &app,
        &host,
        json!([
            { "op": "host_role_add", "role": role },
            { "op": "host_item_set", "kind": "package", "name": "sl", "version": "latest" }
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(state["roles"][0]["name"], "base");
    let config = state["config"].as_str().unwrap();
    assert!(config.contains("\"jq\" = \"latest\""), "{config}");
    assert!(config.contains("\"sl\" = \"latest\""), "{config}");
    assert!(config.contains("\"node\" = \"24\""), "{config}");
    assert!(!config.contains("firefox"), "{config}");

    let (status, body) = host_edits(
        &app,
        &host,
        json!([{ "op": "host_role_add", "role": Uuid::new_v4() }]),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!({ "error": "unknown_role" }))
    );
    let (status, body) = host_edits(
        &app,
        &host,
        json!([{ "op": "host_item_set", "kind": "tool", "name": "node", "version": "absent" }]),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!({ "error": "invalid_edit" }))
    );
    let (status, state) = host_edits(
        &app,
        &host,
        json!([{ "op": "host_role_remove", "role": role }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["roles"], json!([]));
    let kinds = events(&pool, &host).await;
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| kind.starts_with("host.") && kind.ends_with("_set"))
            .cloned()
            .collect::<Vec<_>>(),
        ["host.roles_set", "host.items_set", "host.roles_set"]
    );
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn the_host_list_has_each_hosts_roles_without_the_built_in_one(pool: PgPool) {
    let app = app(pool);
    let (first, _) = create_host(&app, "build-1").await;
    let (_second, _) = create_host(&app, "build-2").await;
    let web = create_role(&app, "web").await;
    let db = create_role(&app, "db").await;
    let (status, body) = host_edits(
        &app,
        &first,
        json!([{ "op": "host_role_add", "role": web }, { "op": "host_role_add", "role": db }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, hosts) = json_call(&app, ADMIN, "GET", "/api/hosts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        hosts[0]["roles"],
        json!([{ "id": db, "name": "db" }, { "id": web, "name": "web" }])
    );
    assert_eq!(hosts[1]["roles"], json!([]));
    assert_eq!(hosts[0]["name"], "build-1");
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn the_agent_gets_its_config_and_reports_plans(pool: PgPool) {
    let app = app(pool.clone());
    let (host, token) = create_host(&app, "build-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    let instance = Uuid::new_v4();
    let role = create_role(&app, "base").await;
    host_edits(
        &app,
        &host,
        json!([{ "op": "host_role_add", "role": role }]),
    )
    .await;
    save_role(
        &app,
        &role,
        json!({ "name": "base", "items": [{ "kind": "package", "name": "jq", "version": "latest" }] }),
    )
    .await;

    let (_, body) = opamp(&app, &credential, &report(instance, true)).await;
    let remote = ServerToAgent::decode(&body[..])
        .unwrap()
        .remote_config
        .unwrap();
    let configs = remote.config.unwrap().config_map;
    assert_eq!(configs.keys().collect::<Vec<_>>(), ["current"]);
    let current = &configs["current"];
    assert_eq!(current.content_type, "application/toml");
    let body = String::from_utf8_lossy(&current.body).into_owned();
    assert!(body.contains("\"jq\" = \"latest\""), "{body}");
    assert_eq!(body, host_state(&app, &host).await["config"]);

    // An agent that has the config gets it no more.
    let mut known = report(instance, false);
    known.remote_config_status = Some(RemoteConfigStatus {
        last_remote_config_hash: remote.config_hash.clone(),
        ..Default::default()
    });
    let (_, answer) = opamp(&app, &credential, &known).await;
    assert!(
        ServerToAgent::decode(&answer[..])
            .unwrap()
            .remote_config
            .is_none()
    );

    let config_hash = sewa_controller::desired::hash(&body);
    let plan = json!({
        "resources": [{ "id": { "kind": "package", "name": "pacman:jq" }, "action": "create" }],
        "summary": { "create": 1 }
    });
    let (status, _) = opamp(
        &app,
        &credential,
        &plan_message(
            instance,
            json!({ "config": "current", "config_hash": config_hash, "plan_hash": "p1", "exit_code": 2, "plan": plan, "error": null }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let state = host_state(&app, &host).await;
    assert_eq!(state["drift"]["plan"], plan);
    assert_eq!(state["drift"]["stale"], false);
    // A plan of a config from before syncs is dropped.
    let (status, _) = opamp(
        &app,
        &credential,
        &plan_message(
            instance,
            json!({ "config": format!("change/{}", Uuid::new_v4()), "config_hash": "x", "plan_hash": "p9", "exit_code": 2, "plan": {}, "error": null }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Another save changes the config, so the plan is stale until the
    // agent plans again.
    save_role(
        &app,
        &role,
        json!({ "name": "base", "items": [{ "kind": "package", "name": "git", "version": "latest" }] }),
    )
    .await;
    assert_eq!(host_state(&app, &host).await["drift"]["stale"], true);
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn drift_is_recorded_once_and_undeclared_packages_show(pool: PgPool) {
    let app = app(pool.clone());
    let (host, token) = create_host(&app, "node-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    let instance = Uuid::new_v4();
    opamp(&app, &credential, &report(instance, true)).await;
    // The approved state is written here directly.
    let role = create_role(&app, "base").await;
    sqlx::query("INSERT INTO role_items VALUES ($1::uuid, 'package', 'jq', 'latest')")
        .bind(&role)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO host_roles VALUES ($1::uuid, $2::uuid)")
        .bind(&host)
        .bind(&role)
        .execute(&pool)
        .await
        .unwrap();

    let current = |exit_code: i32, plan_hash: &str| {
        plan_message(
            instance,
            json!({
                "config": "current", "config_hash": "c", "plan_hash": plan_hash,
                "exit_code": exit_code, "plan": { "resources": [] }, "error": null,
                "explicit": { "manager": "pacman", "packages": ["git", "jq", "sl"] }
            }),
        )
    };
    for message in [
        current(2, "a"),
        current(2, "a"),
        current(0, "z"),
        current(2, "a"),
        current(2, "b"),
    ] {
        assert_eq!(opamp(&app, &credential, &message).await.0, StatusCode::OK);
    }
    let drifts = events(&pool, &host)
        .await
        .into_iter()
        .filter(|kind| kind == "drift.detected")
        .count();
    assert_eq!(drifts, 3);

    let (_, state) = json_call(
        &app,
        ADMIN,
        "GET",
        &format!("/api/hosts/{host}/state"),
        None,
    )
    .await;
    assert_eq!(state["roles"][0]["name"], "base");
    assert_eq!(state["drift"]["status"], "drift");
    assert_eq!(state["drift"]["plan_hash"], "b");
    assert_eq!(state["undeclared"]["manager"], "pacman");
    assert_eq!(state["undeclared"]["packages"], json!(["git", "sl"]));
    assert_eq!(state["undeclared"]["explicit_count"], 3);
    // A plan sent when it was made is not marked late.
    assert_eq!(state["drift"]["received_at"], Value::Null);
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn reports_kept_while_offline_keep_their_time(pool: PgPool) {
    let app = app(pool.clone());
    let (host, token) = create_host(&app, "web-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    let instance = Uuid::new_v4();
    opamp(&app, &credential, &report(instance, true)).await;
    // The controller was down for an hour.
    let last_seen: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "UPDATE hosts SET last_seen_at = now() - interval '1 hour' RETURNING last_seen_at",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let now = chrono::Utc::now();
    let ago = |minutes: i64| now - chrono::Duration::minutes(minutes);
    let plan = |exit_code: i32, plan_hash: &str, at: chrono::DateTime<chrono::Utc>| {
        plan_message(
            instance,
            json!({
                "config": "current", "config_hash": "c", "plan_hash": plan_hash,
                "exit_code": exit_code, "plan": { "summary": { "drifted": 1 } },
                "error": null,
                "explicit": { "manager": "pacman", "packages": ["sl"] },
                "at": at.timestamp_millis(),
            }),
        )
    };
    // The outbox sends the drift, then the agent's later plan of the same
    // drift. An older plan, out of order, changes nothing.
    for message in [
        plan(2, "a", ago(30)),
        plan(2, "a", ago(20)),
        plan(0, "z", ago(40)),
    ] {
        assert_eq!(opamp(&app, &credential, &message).await.0, StatusCode::OK);
    }

    let (_, state) = json_call(
        &app,
        ADMIN,
        "GET",
        &format!("/api/hosts/{host}/state"),
        None,
    )
    .await;
    assert_eq!(state["drift"]["status"], "drift");
    let time = |value: &Value| {
        chrono::DateTime::parse_from_rfc3339(value.as_str().unwrap())
            .unwrap()
            .timestamp_millis()
    };
    assert_eq!(
        time(&state["drift"]["reported_at"]),
        ago(20).timestamp_millis()
    );
    assert!(time(&state["drift"]["received_at"]) >= now.timestamp_millis());
    assert_eq!(
        time(&state["undeclared"]["reported_at"]),
        ago(20).timestamp_millis()
    );

    // The history reads newest first by the time things happened, so the
    // drift the host found during the silence comes after the silence.
    let (_, history) = json_call(
        &app,
        ADMIN,
        "GET",
        &format!("/api/hosts/{host}/events"),
        None,
    )
    .await;
    let history = history.as_array().unwrap();
    assert!(history.is_sorted_by(|a, b| time(&a["at"]) >= time(&b["at"])));
    let kinds: Vec<&str> = history
        .iter()
        .map(|event| event["kind"].as_str().unwrap())
        .filter(|kind| ["drift.detected", "host.offline"].contains(kind))
        .collect();
    assert_eq!(kinds, ["drift.detected", "host.offline"]);
    let find = |kind: &str| history.iter().find(|event| event["kind"] == kind).unwrap();
    let drift = find("drift.detected");
    assert_eq!(time(&drift["at"]), ago(30).timestamp_millis());
    assert!(time(&drift["data"]["received_at"]) >= now.timestamp_millis());
    let offline = find("host.offline");
    assert_eq!(time(&offline["at"]), last_seen.timestamp_millis());
    assert_eq!(time(&offline["data"]["from"]), last_seen.timestamp_millis());

    // A clock ahead of the controller's does not date a report in the
    // future.
    let ahead = now + chrono::Duration::hours(1);
    opamp(&app, &credential, &plan(0, "z", ahead)).await;
    let (_, state) = json_call(
        &app,
        ADMIN,
        "GET",
        &format!("/api/hosts/{host}/state"),
        None,
    )
    .await;
    assert!(time(&state["drift"]["reported_at"]) <= chrono::Utc::now().timestamp_millis());
    assert_eq!(state["drift"]["received_at"], Value::Null);
}

/// The relying party of the test controller's public URL.
const RP_ID: &str = "controller.example";
const ORIGIN: &str = "http://controller.example:8080";

/// Registers a passkey of the admin, as the browser does with
/// `navigator.credentials.create`.
async fn register_passkey(app: &Router, key: &Authenticator, origin: &str) -> (StatusCode, Value) {
    let (status, options) = json_call(app, ADMIN, "POST", "/api/passkeys/options", None).await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(options["rp"]["id"], RP_ID);
    assert_eq!(options["algorithm"], -7);
    let challenge = sewa_approval::decode(options["challenge"].as_str().unwrap()).unwrap();
    let client_data = Authenticator::client_data("webauthn.create", &challenge, origin);
    let mut authenticator_data = Sha256::digest(RP_ID.as_bytes()).to_vec();
    authenticator_data.extend_from_slice(&[0x45, 0, 0, 0, 0]);
    json_call(
        app,
        ADMIN,
        "POST",
        "/api/passkeys",
        Some(json!({
            "name": "laptop",
            "credential_id": sewa_approval::encode(&key.id),
            "public_key": key.public_key(),
            "algorithm": -7,
            "authenticator_data": sewa_approval::encode(&authenticator_data),
            "client_data_json": sewa_approval::encode(&client_data),
        })),
    )
    .await
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn passkeys_register_and_get_pinned_at_install(pool: PgPool) {
    let app = app(pool.clone());
    let key = Authenticator::new(1);
    // The install script of a controller without passkeys pins none.
    let (_, script) = call(
        &app,
        NODE,
        Request::get("/install").body(Body::empty()).unwrap(),
    )
    .await;
    assert!(String::from_utf8(script).unwrap().contains(r#""keys":[]"#));

    let (status, body) = register_passkey(&app, &key, "https://evil.example.com").await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!({ "error": "wrong_origin" }))
    );
    let (status, created) = register_passkey(&app, &key, ORIGIN).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["login"], "admin@example.com");
    let (status, body) = register_passkey(&app, &key, ORIGIN).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "passkey_exists" }))
    );
    let (_, list) = json_call(&app, ADMIN, "GET", "/api/passkeys", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["credential_id"], sewa_approval::encode(&key.id));

    let (_, script) = call(
        &app,
        NODE,
        Request::get("/install").body(Body::empty()).unwrap(),
    )
    .await;
    let script = String::from_utf8(script).unwrap();
    assert!(script.contains(&key.public_key()), "{script}");
    assert!(script.contains(r#""rp_id":"controller.example""#));
    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM events ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(kinds.contains(&"passkey.registered".to_owned()));
}

/// Sends a plan of the host's current config.
async fn plan_current(
    app: &Router,
    host: &str,
    credential: &str,
    instance: Uuid,
    exit_code: i32,
    plan_hash: &str,
) {
    let config = host_state(app, host).await["config"]
        .as_str()
        .unwrap()
        .to_owned();
    let config_hash = sewa_controller::desired::hash(&config);
    let (status, _) = opamp(
        app,
        credential,
        &plan_message(
            instance,
            json!({ "config": "current", "config_hash": config_hash, "plan_hash": plan_hash, "exit_code": exit_code, "plan": {}, "error": null }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// A connected host in a role that adds `jq`, and the agent's plan of it.
/// Returns the host, the credential, and the instance.
async fn planned_host(app: &Router, name: &str) -> (String, String, Uuid) {
    let (host, token) = create_host(app, name).await;
    let (_, credential) = enroll(app, NODE, &token).await;
    let instance = Uuid::new_v4();
    opamp(app, &credential, &report(instance, true)).await;
    let role = create_role(app, &format!("{name}-role")).await;
    save_role(
        app,
        &role,
        json!({ "name": format!("{name}-role"), "items": [{ "kind": "package", "name": "jq", "version": "latest" }] }),
    )
    .await;
    host_edits(app, &host, json!([{ "op": "host_role_add", "role": role }])).await;
    plan_current(app, &host, &credential, instance, 2, "p1").await;
    (host, credential, instance)
}

async fn start_sync(app: &Router, hosts: Value) -> String {
    let (status, sync) = json_call(
        app,
        ADMIN,
        "POST",
        "/api/syncs",
        Some(json!({ "hosts": hosts })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sync}");
    sync["id"].as_str().unwrap().to_owned()
}

async fn get_sync(app: &Router, id: &str) -> Value {
    let (status, sync) = json_call(app, ADMIN, "GET", &format!("/api/syncs/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{sync}");
    sync
}

fn outcome_message(instance: Uuid, outcome: &Outcome) -> AgentToServer {
    AgentToServer {
        custom_message: Some(CustomMessage {
            capability: "io.sewa.approval".to_owned(),
            r#type: "result".to_owned(),
            data: serde_json::to_vec(outcome).unwrap(),
        }),
        ..report(instance, false)
    }
}

/// The approval the controller sends with its answer, if any.
async fn sent_approval(app: &Router, credential: &str, instance: Uuid) -> Option<Signed> {
    let (_, body) = opamp(app, credential, &report(instance, false)).await;
    let custom = ServerToAgent::decode(&body[..]).unwrap().custom_message?;
    assert_eq!(custom.capability, "io.sewa.approval");
    assert_eq!(custom.r#type, "approval");
    Some(serde_json::from_slice(&custom.data).unwrap())
}

async fn approve_with(
    app: &Router,
    id: &str,
    key: &Authenticator,
    hosts: Option<Value>,
) -> (StatusCode, Value) {
    let only = json!({ "hosts": hosts });
    let (status, options) = json_call(
        app,
        ADMIN,
        "POST",
        &format!("/api/syncs/{id}/approval"),
        Some(only),
    )
    .await;
    if status != StatusCode::OK {
        return (status, options);
    }
    let challenge = sewa_approval::decode(options["challenge"].as_str().unwrap()).unwrap();
    let mut body = serde_json::to_value(key.assert(RP_ID, ORIGIN, &challenge)).unwrap();
    body["approval_id"] = options["approval_id"].clone();
    body["hosts"] = hosts.unwrap_or(Value::Null);
    json_call(
        app,
        ADMIN,
        "POST",
        &format!("/api/syncs/{id}/approve"),
        Some(body),
    )
    .await
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn a_sync_goes_to_the_agents_and_back(pool: PgPool) {
    let app = app(pool.clone());
    let (host, credential, instance) = planned_host(&app, "build-1").await;
    // A host already in sync, and one that never planned.
    let (quiet, quiet_credential, quiet_instance) = planned_host(&app, "build-2").await;
    plan_current(&app, &quiet, &quiet_credential, quiet_instance, 0, "p0").await;
    let (silent, token) = create_host(&app, "build-3").await;
    enroll(&app, NODE, &token).await;
    let (pending, _) = create_host(&app, "build-4").await;

    for (hosts, error) in [
        (json!([]), "no_hosts"),
        (json!([pending]), "unknown_host"),
        (json!([Uuid::new_v4()]), "unknown_host"),
    ] {
        let (status, body) = json_call(
            &app,
            ADMIN,
            "POST",
            "/api/syncs",
            Some(json!({ "hosts": hosts })),
        )
        .await;
        assert_eq!(
            (status, body),
            (StatusCode::BAD_REQUEST, json!({ "error": error }))
        );
    }
    let id = start_sync(&app, json!([host, quiet, silent])).await;
    let sync = get_sync(&app, &id).await;
    assert_eq!(sync["status"], "ready", "{sync}");
    assert_eq!(sync["created_by"], "admin@example.com");
    let steps: Vec<&Value> = sync["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| &h["step"])
        .collect();
    assert_eq!(steps, ["picked", "picked", "picked"]);
    assert_eq!(sync["hosts"][0]["plan"]["plan_hash"], "p1");
    let (_, syncs) = json_call(&app, ADMIN, "GET", "/api/syncs", None).await;
    assert_eq!(syncs[0]["id"], id);
    assert_eq!(syncs[0]["host_count"], 3);

    let approval_uri = format!("/api/syncs/{id}/approval");
    let (status, body) = json_call(&app, ADMIN, "POST", &approval_uri, None).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "no_passkey" }))
    );
    let key = Authenticator::new(1);
    register_passkey(&app, &key, ORIGIN).await;

    let (status, options) = json_call(&app, ADMIN, "POST", &approval_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{options}");
    // The approval keeps its wire format: `change_id` carries the sync id.
    assert_eq!(options["change_id"], id);
    assert_eq!(options["hosts"].as_array().unwrap().len(), 1);
    assert_eq!(options["hosts"][0]["host_id"], host);
    assert_eq!(options["hosts"][0]["instance_uid"], instance.to_string());
    assert_eq!(options["hosts"][0]["plan_hash"], "p1");
    assert!(
        options["signed_text"]
            .as_str()
            .unwrap()
            .starts_with(&format!(
                "sewa approval v1\napproval {}\nchange {id}\n",
                options["approval_id"].as_str().unwrap()
            ))
    );
    // An approve body: the assertion and the id of the options it signs.
    let approve_body = |assertion: &sewa_approval::Assertion| {
        let mut body = serde_json::to_value(assertion).unwrap();
        body["approval_id"] = options["approval_id"].clone();
        body
    };
    let challenge = sewa_approval::decode(options["challenge"].as_str().unwrap()).unwrap();
    let approve_uri = format!("/api/syncs/{id}/approve");
    // A key the admin did not register, and a signature of other plans.
    let forged = Authenticator::new(2).assert(RP_ID, ORIGIN, &challenge);
    let (status, body) = json_call(
        &app,
        ADMIN,
        "POST",
        &approve_uri,
        Some(approve_body(&forged)),
    )
    .await;
    assert_eq!(
        (status, body),
        (
            StatusCode::BAD_REQUEST,
            json!({ "error": "invalid_assertion" })
        )
    );
    let stale = key.assert(RP_ID, ORIGIN, &[0; 32]);
    let (status, body) = json_call(
        &app,
        ADMIN,
        "POST",
        &approve_uri,
        Some(approve_body(&stale)),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "plans_changed" }))
    );
    assert!(sent_approval(&app, &credential, instance).await.is_none());

    let assertion = key.assert(RP_ID, ORIGIN, &challenge);
    let (status, sync) = json_call(
        &app,
        ADMIN,
        "POST",
        &approve_uri,
        Some(approve_body(&assertion)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sync}");
    assert_eq!(sync["status"], "applying");
    let steps: Vec<&Value> = sync["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| &h["step"])
        .collect();
    assert_eq!(steps, ["waiting", "in_sync", "skipped"]);
    assert_eq!(sync["approvals"][0]["by"], "admin@example.com");
    assert_eq!(sync["approvals"][0]["host_count"], 1);

    // The agent gets the approval on every answer until it reports, and
    // it verifies against the pinned keys.
    let signed = sent_approval(&app, &credential, instance).await.unwrap();
    assert_eq!(signed.approval.change_id.to_string(), id);
    let pinned = Pinned {
        rp_id: RP_ID.to_owned(),
        origin: ORIGIN.to_owned(),
        keys: vec![key.approver("admin@example.com")],
    };
    assert!(
        sewa_approval::verify(&pinned, &signed.assertion, &signed.approval.challenge()).is_ok()
    );
    assert!(sent_approval(&app, &credential, instance).await.is_some());
    assert!(
        sent_approval(&app, &quiet_credential, quiet_instance)
            .await
            .is_none()
    );
    let outcome = Outcome {
        approval_id: signed.approval.approval_id,
        change_id: signed.approval.change_id,
        status: "applied".to_owned(),
        plan_hash: Some("p1".to_owned()),
        log: "installed jq".to_owned(),
        // The agent kept the result while the controller was down.
        at: Some((chrono::Utc::now() - chrono::Duration::minutes(5)).timestamp_millis()),
    };
    opamp(&app, &credential, &outcome_message(instance, &outcome)).await;
    assert!(sent_approval(&app, &credential, instance).await.is_none());
    let sync = get_sync(&app, &id).await;
    assert_eq!(sync["status"], "done");
    let result = &sync["hosts"][0]["result"];
    assert_eq!(sync["hosts"][0]["step"], "applied");
    assert_eq!(result["log"], "installed jq");
    let reported = chrono::DateTime::parse_from_rfc3339(result["reported_at"].as_str().unwrap())
        .unwrap()
        .timestamp_millis();
    assert_eq!(Some(reported), outcome.at);
    assert!(result["received_at"].is_string());
    let kinds = events(&pool, &host).await;
    assert!(kinds.ends_with(&["sync.approved".to_owned(), "plan.applied".to_owned()]));
    let (_, edits) = json_call(&app, ADMIN, "GET", "/api/syncs/edits", None).await;
    assert_eq!(edits["since"], sync["created_at"]);
    assert_eq!(edits["edits"], json!([]));
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn a_signature_signs_once(pool: PgPool) {
    let app = app(pool.clone());
    let (host, credential, instance) = planned_host(&app, "build-1").await;
    let key = Authenticator::new(1);
    register_passkey(&app, &key, ORIGIN).await;
    let id = start_sync(&app, json!([host])).await;
    let (_, options) = json_call(
        &app,
        ADMIN,
        "POST",
        &format!("/api/syncs/{id}/approval"),
        None,
    )
    .await;
    let challenge = sewa_approval::decode(options["challenge"].as_str().unwrap()).unwrap();
    let mut body = serde_json::to_value(key.assert(RP_ID, ORIGIN, &challenge)).unwrap();
    body["approval_id"] = options["approval_id"].clone();
    let approve_uri = format!("/api/syncs/{id}/approve");
    let (status, _) = json_call(&app, ADMIN, "POST", &approve_uri, Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK);
    // The apply fails and changes nothing, so the same plan is up again.
    let signed = sent_approval(&app, &credential, instance).await.unwrap();
    let failed = Outcome {
        approval_id: signed.approval.approval_id,
        change_id: signed.approval.change_id,
        status: "failed".to_owned(),
        plan_hash: Some("p1".to_owned()),
        log: "failed".to_owned(),
        at: None,
    };
    opamp(&app, &credential, &outcome_message(instance, &failed)).await;
    let (status, body) = json_call(&app, ADMIN, "POST", &approve_uri, Some(body)).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "plans_changed" }))
    );
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn a_failed_or_replanned_host_is_signed_again(pool: PgPool) {
    let app = app(pool.clone());
    let (host, credential, instance) = planned_host(&app, "build-1").await;
    let (other, other_credential, other_instance) = planned_host(&app, "build-2").await;
    let key = Authenticator::new(1);
    register_passkey(&app, &key, ORIGIN).await;
    let id = start_sync(&app, json!([host, other])).await;
    let (status, sync) = approve_with(&app, &id, &key, None).await;
    assert_eq!(status, StatusCode::OK, "{sync}");

    // One host's plan changed before it applied; the other failed.
    let signed = sent_approval(&app, &credential, instance).await.unwrap();
    let replan = Outcome {
        approval_id: signed.approval.approval_id,
        change_id: signed.approval.change_id,
        status: "replan".to_owned(),
        plan_hash: Some("p2".to_owned()),
        log: "the plan changed since the approval".to_owned(),
        at: None,
    };
    opamp(&app, &credential, &outcome_message(instance, &replan)).await;
    let other_signed = sent_approval(&app, &other_credential, other_instance)
        .await
        .unwrap();
    let failed = Outcome {
        approval_id: other_signed.approval.approval_id,
        change_id: other_signed.approval.change_id,
        status: "failed".to_owned(),
        plan_hash: Some("p1".to_owned()),
        log: "pacman: target not found: jq".to_owned(),
        at: None,
    };
    opamp(
        &app,
        &other_credential,
        &outcome_message(other_instance, &failed),
    )
    .await;
    let sync = get_sync(&app, &id).await;
    let steps: Vec<&Value> = sync["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| &h["step"])
        .collect();
    assert_eq!(steps, ["replan", "failed"]);
    assert_eq!(
        sync["hosts"][1]["result"]["log"],
        "pacman: target not found: jq"
    );

    // Retry one host: only it is signed.
    plan_current(&app, &host, &credential, instance, 2, "p2").await;
    let (status, sync) = approve_with(&app, &id, &key, Some(json!([host]))).await;
    assert_eq!(status, StatusCode::OK, "{sync}");
    let steps: Vec<&Value> = sync["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| &h["step"])
        .collect();
    assert_eq!(steps, ["waiting", "failed"]);
    let again = sent_approval(&app, &credential, instance).await.unwrap();
    assert_ne!(again.approval.approval_id, signed.approval.approval_id);
    assert_eq!(again.approval.hosts.len(), 1);
    assert_eq!(again.approval.hosts[0].plan_hash, "p2");
    assert!(
        sent_approval(&app, &other_credential, other_instance)
            .await
            .is_none()
    );
    let (status, sync) = approve_with(&app, &id, &key, Some(json!([other]))).await;
    assert_eq!(status, StatusCode::OK, "{sync}");
    assert_eq!(sync["hosts"][1]["step"], "waiting");
    assert_eq!(sync["hosts"][1]["result"]["log"], Value::Null);
    // Nothing is left to sign.
    let (status, body) = approve_with(&app, &id, &key, None).await;
    assert_eq!(
        (status, body),
        (
            StatusCode::CONFLICT,
            json!({ "error": "nothing_to_approve" })
        )
    );
    let kinds = events(&pool, &host).await;
    assert_eq!(
        kinds.iter().filter(|kind| *kind == "sync.approved").count(),
        2
    );
    assert!(kinds.contains(&"apply.replan".to_owned()));
    assert!(
        events(&pool, &other)
            .await
            .contains(&"apply.failed".to_owned())
    );
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn drift_can_be_adopted_or_ignored(pool: PgPool) {
    let app = app(pool.clone());
    let (host, token) = create_host(&app, "build-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    let instance = Uuid::new_v4();
    opamp(&app, &credential, &report(instance, true)).await;
    let config = host_state(&app, &host).await["config"]
        .as_str()
        .unwrap()
        .to_owned();
    let current = sewa_controller::desired::hash(&config);
    opamp(
        &app,
        &credential,
        &plan_message(
            instance,
            json!({
                "config": "current", "config_hash": current, "plan_hash": "d1",
                "exit_code": 2, "plan": { "resources": [] }, "error": null,
                "explicit": { "manager": "pacman", "packages": ["git", "mise", "sl", "yay"] }
            }),
        ),
    )
    .await;
    let drift = format!("/api/hosts/{host}/drift");
    // The bootstrap role declares mise.
    assert_eq!(
        host_state(&app, &host).await["undeclared"]["packages"],
        json!(["git", "sl", "yay"])
    );

    let (status, _) = json_call(
        &app,
        ADMIN,
        "POST",
        &format!("{drift}/adopt"),
        Some(json!({ "package": "sl", "role": null })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let role = create_role(&app, "base").await;
    let (status, _) = json_call(
        &app,
        ADMIN,
        "POST",
        &format!("{drift}/adopt"),
        Some(json!({ "package": "git", "role": role })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = json_call(
        &app,
        ADMIN,
        "POST",
        &format!("{drift}/adopt"),
        Some(json!({ "package": "git", "role": Uuid::new_v4() })),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!({ "error": "unknown_role" }))
    );
    let state = host_state(&app, &host).await;
    assert_eq!(
        state["adjustments"],
        json!([{ "action": "set", "kind": "package", "name": "sl", "version": "latest" }]),
        "{state}"
    );
    let (_, base) = json_call(&app, ADMIN, "GET", &format!("/api/roles/{role}"), None).await;
    assert_eq!(base["items"][0]["name"], "git");

    let (status, _) = json_call(&app, ADMIN, "POST", &format!("{drift}/ignore"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let state = host_state(&app, &host).await;
    // The adjustment changed the config, so this drift is of an older plan.
    assert_eq!(state["drift"]["stale"], true);
    assert_eq!(state["undeclared"]["packages"], json!([]));
    let kinds = events(&pool, &host).await;
    for kind in ["drift.adopted", "drift.ignored"] {
        assert!(kinds.contains(&kind.to_owned()), "{kind} in {kinds:?}");
    }
}
