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

async fn edit(app: &Router, edit: Value) -> (StatusCode, Value) {
    json_call(app, ADMIN, "POST", "/api/changes/draft/edits", Some(edit)).await
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
async fn edits_go_into_one_draft_change(pool: PgPool) {
    let app = app(pool.clone());
    let (host, _) = create_host(&app, "node-1").await;
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

    let (status, change) = edit(
        &app,
        json!({ "op": "role_item_set", "role": role, "kind": "package", "name": "jq", "version": "latest" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{change}");
    assert_eq!(change["status"], "draft");
    let (_, second) = edit(
        &app,
        json!({ "op": "host_role_add", "host": host, "role": role }),
    )
    .await;
    assert_eq!(second["id"], change["id"]);
    assert_eq!(second["edit_count"], 2);

    let (status, body) = edit(
        &app,
        json!({ "op": "role_item_set", "role": Uuid::new_v4(), "kind": "package", "name": "jq", "version": "latest" }),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!({ "error": "unknown_role" }))
    );
    let (status, body) = edit(
        &app,
        json!({ "op": "role_item_set", "role": role, "kind": "tool", "name": "node", "version": "absent" }),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::BAD_REQUEST, json!({ "error": "invalid_item" }))
    );

    // The approved state is untouched; the draft shows what it proposes.
    let (_, roles) = json_call(&app, ADMIN, "GET", "/api/roles", None).await;
    assert_eq!(roles[0]["items"], json!([]));
    assert_eq!(
        roles[0]["proposed_items"],
        json!([{ "kind": "package", "name": "jq", "version": "latest" }])
    );
    assert_eq!(roles[0]["proposed_hosts"][0]["name"], "node-1");
    let (_, state) = json_call(
        &app,
        ADMIN,
        "GET",
        &format!("/api/hosts/{host}/state"),
        None,
    )
    .await;
    assert_eq!(state["roles"], json!([]));
    assert_eq!(state["proposed_roles"][0]["name"], "base");

    let id = change["id"].as_str().unwrap();
    let (status, detail) = json_call(&app, ADMIN, "GET", &format!("/api/changes/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["edits"].as_array().unwrap().len(), 2);
    assert_eq!(detail["hosts"][0]["name"], "node-1");
    assert!(
        detail["hosts"][0]["proposed_config"]
            .as_str()
            .unwrap()
            .contains("[bootstrap.packages]\n\"jq\" = \"latest\"\n")
    );
    assert_eq!(detail["hosts"][0]["plan"], Value::Null);

    let discard = format!("/api/changes/{id}/discard");
    let (status, closed) = json_call(&app, ADMIN, "POST", &discard, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(closed["status"], "discarded");
    let (status, body) = json_call(&app, ADMIN, "POST", &discard, None).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "not_draft" }))
    );
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM events WHERE change_id = $1::uuid ORDER BY id")
            .bind(id)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        kinds,
        [
            "change.created",
            "change.edited",
            "change.edited",
            "change.discarded"
        ]
    );
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn the_agent_gets_configs_and_reports_plans(pool: PgPool) {
    let app = app(pool.clone());
    let (host, token) = create_host(&app, "node-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    let instance = Uuid::new_v4();
    let role = create_role(&app, "base").await;
    edit(
        &app,
        json!({ "op": "host_role_add", "host": host, "role": role }),
    )
    .await;
    let (_, change) = edit(
        &app,
        json!({ "op": "role_item_set", "role": role, "kind": "package", "name": "jq", "version": "latest" }),
    )
    .await;
    let id = change["id"].as_str().unwrap();
    let name = format!("change/{id}");

    let (_, body) = opamp(&app, &credential, &report(instance, true)).await;
    let remote = ServerToAgent::decode(&body[..])
        .unwrap()
        .remote_config
        .unwrap();
    let configs = remote.config.unwrap().config_map;
    assert_eq!(configs.len(), 2);
    assert!(!String::from_utf8_lossy(&configs["current"].body).contains("jq"));
    let proposed = &configs[&name];
    assert_eq!(proposed.content_type, "application/toml");
    assert!(String::from_utf8_lossy(&proposed.body).contains("\"jq\" = \"latest\""));

    // An agent that has the config gets it no more.
    let mut known = report(instance, false);
    known.remote_config_status = Some(RemoteConfigStatus {
        last_remote_config_hash: remote.config_hash.clone(),
        ..Default::default()
    });
    let (_, body) = opamp(&app, &credential, &known).await;
    assert!(
        ServerToAgent::decode(&body[..])
            .unwrap()
            .remote_config
            .is_none()
    );

    let config_hash = sewa_controller::desired::hash(&String::from_utf8_lossy(&proposed.body));
    let plan = json!({
        "resources": [{ "id": { "kind": "package", "name": "pacman:jq" }, "action": "create" }],
        "summary": { "create": 1 }
    });
    let (status, _) = opamp(
        &app,
        &credential,
        &plan_message(
            instance,
            json!({ "config": name, "config_hash": config_hash, "plan_hash": "p1", "exit_code": 2, "plan": plan, "error": null }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, detail) = json_call(&app, ADMIN, "GET", &format!("/api/changes/{id}"), None).await;
    let reported = &detail["hosts"][0]["plan"];
    assert_eq!(reported["plan"], plan);
    assert_eq!(reported["exit_code"], 2);
    assert_eq!(reported["stale"], false);

    // Another edit changes the config, so the plan is stale until the
    // agent plans again.
    edit(
        &app,
        json!({ "op": "role_item_set", "role": role, "kind": "package", "name": "git", "version": "latest" }),
    )
    .await;
    let (_, detail) = json_call(&app, ADMIN, "GET", &format!("/api/changes/{id}"), None).await;
    assert_eq!(detail["hosts"][0]["plan"]["stale"], true);
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

/// A connected host in a role, with a draft that adds `jq` to the role,
/// and the agent's plan of it. Returns the host, the credential, the
/// instance, and the change id.
async fn planned_draft(app: &Router) -> (String, String, Uuid, String) {
    let (host, token) = create_host(app, "node-1").await;
    let (_, credential) = enroll(app, NODE, &token).await;
    let instance = Uuid::new_v4();
    opamp(app, &credential, &report(instance, true)).await;
    let role = create_role(app, "base").await;
    edit(
        app,
        json!({ "op": "host_role_add", "host": host, "role": role }),
    )
    .await;
    let (_, change) = edit(
        app,
        json!({ "op": "role_item_set", "role": role, "kind": "package", "name": "jq", "version": "latest" }),
    )
    .await;
    let id = change["id"].as_str().unwrap().to_owned();
    let (_, body) = opamp(app, &credential, &report(instance, false)).await;
    let configs = ServerToAgent::decode(&body[..])
        .unwrap()
        .remote_config
        .unwrap()
        .config
        .unwrap()
        .config_map;
    let name = format!("change/{id}");
    let config_hash =
        sewa_controller::desired::hash(&String::from_utf8_lossy(&configs[&name].body));
    opamp(
        app,
        &credential,
        &plan_message(
            instance,
            json!({ "config": name, "config_hash": config_hash, "plan_hash": "p1", "exit_code": 2, "plan": {}, "error": null }),
        ),
    )
    .await;
    (host, credential, instance, id)
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

async fn approve_with(app: &Router, id: &str, key: &Authenticator) -> (StatusCode, Value) {
    let (status, options) = json_call(
        app,
        ADMIN,
        "POST",
        &format!("/api/changes/{id}/approval"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    let challenge = sewa_approval::decode(options["challenge"].as_str().unwrap()).unwrap();
    let assertion = key.assert(RP_ID, ORIGIN, &challenge);
    json_call(
        app,
        ADMIN,
        "POST",
        &format!("/api/changes/{id}/approve"),
        Some(serde_json::to_value(&assertion).unwrap()),
    )
    .await
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn an_approved_change_goes_to_the_agent_and_back(pool: PgPool) {
    let app = app(pool.clone());
    let (host, credential, instance, id) = planned_draft(&app).await;
    let approval_uri = format!("/api/changes/{id}/approval");
    let (status, body) = json_call(&app, ADMIN, "POST", &approval_uri, None).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "no_passkey" }))
    );
    let key = Authenticator::new(1);
    register_passkey(&app, &key, ORIGIN).await;

    let (status, options) = json_call(&app, ADMIN, "POST", &approval_uri, None).await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(options["change_id"], id);
    assert_eq!(options["hosts"][0]["host_id"], host);
    assert_eq!(options["hosts"][0]["instance_uid"], instance.to_string());
    assert_eq!(options["hosts"][0]["plan_hash"], "p1");
    assert!(
        options["signed_text"]
            .as_str()
            .unwrap()
            .starts_with(&format!("sewa approval v1\nchange {id}\n"))
    );
    let challenge = sewa_approval::decode(options["challenge"].as_str().unwrap()).unwrap();
    let approve_uri = format!("/api/changes/{id}/approve");
    // A key the admin did not register, and a signature of other plans.
    let forged = Authenticator::new(2).assert(RP_ID, ORIGIN, &challenge);
    let (status, body) = json_call(
        &app,
        ADMIN,
        "POST",
        &approve_uri,
        Some(serde_json::to_value(&forged).unwrap()),
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
        Some(serde_json::to_value(&stale).unwrap()),
    )
    .await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "plans_changed" }))
    );
    assert!(sent_approval(&app, &credential, instance).await.is_none());

    let assertion = key.assert(RP_ID, ORIGIN, &challenge);
    let (status, change) = json_call(
        &app,
        ADMIN,
        "POST",
        &approve_uri,
        Some(serde_json::to_value(&assertion).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{change}");
    assert_eq!(change["status"], "approved");
    // The edits are the approved state now.
    let (_, roles) = json_call(&app, ADMIN, "GET", "/api/roles", None).await;
    assert_eq!(roles[0]["items"][0]["name"], "jq");
    let (_, detail) = json_call(&app, ADMIN, "GET", &format!("/api/changes/{id}"), None).await;
    assert_eq!(detail["hosts"][0]["result"]["status"], "waiting");
    assert_eq!(detail["approvals"][0]["by"], "admin@example.com");

    // The agent gets the approval with the new current config, on every
    // answer until it reports, and it verifies against the pinned keys.
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
    let outcome = Outcome {
        approval_id: signed.approval_id,
        change_id: signed.approval.change_id,
        status: "applied".to_owned(),
        plan_hash: Some("p1".to_owned()),
        log: "installed jq".to_owned(),
        // The agent kept the result while the controller was down.
        at: Some((chrono::Utc::now() - chrono::Duration::minutes(5)).timestamp_millis()),
    };
    opamp(&app, &credential, &outcome_message(instance, &outcome)).await;
    assert!(sent_approval(&app, &credential, instance).await.is_none());
    let (_, detail) = json_call(&app, ADMIN, "GET", &format!("/api/changes/{id}"), None).await;
    assert_eq!(detail["hosts"][0]["result"]["status"], "applied");
    assert_eq!(detail["hosts"][0]["result"]["log"], "installed jq");
    let result = &detail["hosts"][0]["result"];
    let reported = chrono::DateTime::parse_from_rfc3339(result["reported_at"].as_str().unwrap())
        .unwrap()
        .timestamp_millis();
    assert_eq!(Some(reported), outcome.at);
    assert!(result["received_at"].is_string());
    let kinds = events(&pool, &host).await;
    assert!(kinds.ends_with(&["change.approved".to_owned(), "plan.applied".to_owned()]));
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn a_host_whose_plan_changed_gets_approved_again(pool: PgPool) {
    let app = app(pool.clone());
    let (host, credential, instance, id) = planned_draft(&app).await;
    let key = Authenticator::new(1);
    register_passkey(&app, &key, ORIGIN).await;
    let (status, _) = approve_with(&app, &id, &key).await;
    assert_eq!(status, StatusCode::OK);
    let signed = sent_approval(&app, &credential, instance).await.unwrap();
    let outcome = Outcome {
        approval_id: signed.approval_id,
        change_id: signed.approval.change_id,
        status: "replan".to_owned(),
        plan_hash: Some("p2".to_owned()),
        log: "the plan changed since the approval".to_owned(),
        at: None,
    };
    opamp(&app, &credential, &outcome_message(instance, &outcome)).await;
    let (_, detail) = json_call(&app, ADMIN, "GET", &format!("/api/changes/{id}"), None).await;
    assert_eq!(detail["hosts"][0]["result"]["status"], "replan");
    assert!(sent_approval(&app, &credential, instance).await.is_none());

    // Until the agent plans its new current config, there is nothing to
    // approve.
    let approval_uri = format!("/api/changes/{id}/approval");
    let (status, body) = json_call(&app, ADMIN, "POST", &approval_uri, None).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, json!({ "error": "plans_not_ready" }))
    );
    let current = signed.approval.hosts[0].config_hash.clone();
    opamp(
        &app,
        &credential,
        &plan_message(
            instance,
            json!({ "config": "current", "config_hash": current, "plan_hash": "p2", "exit_code": 2, "plan": {}, "error": null }),
        ),
    )
    .await;
    let (status, change) = approve_with(&app, &id, &key).await;
    assert_eq!(status, StatusCode::OK, "{change}");
    let again = sent_approval(&app, &credential, instance).await.unwrap();
    assert_ne!(again.approval_id, signed.approval_id);
    assert_eq!(again.approval.hosts[0].plan_hash, "p2");
    let kinds = events(&pool, &host).await;
    assert_eq!(
        kinds
            .iter()
            .filter(|kind| *kind == "change.approved")
            .count(),
        2
    );
    assert!(kinds.contains(&"apply.replan".to_owned()));
}

#[sqlx::test(migrator = "sewa_controller::MIGRATOR")]
async fn drift_can_be_reverted_adopted_or_ignored(pool: PgPool) {
    let app = app(pool.clone());
    let (host, token) = create_host(&app, "node-1").await;
    let (_, credential) = enroll(&app, NODE, &token).await;
    let instance = Uuid::new_v4();
    opamp(&app, &credential, &report(instance, true)).await;
    let current =
        sewa_controller::desired::hash(&sewa_controller::desired::render(&Default::default()));
    opamp(
        &app,
        &credential,
        &plan_message(
            instance,
            json!({
                "config": "current", "config_hash": current, "plan_hash": "d1",
                "exit_code": 2, "plan": { "resources": [] }, "error": null,
                "explicit": { "manager": "pacman", "packages": ["git", "sl"] }
            }),
        ),
    )
    .await;
    let drift = format!("/api/hosts/{host}/drift");

    let (status, revert) = json_call(&app, ADMIN, "POST", &format!("{drift}/revert"), None).await;
    assert_eq!(status, StatusCode::CREATED, "{revert}");
    assert_eq!(revert["revert_host"]["id"], host);
    let (status, again) = json_call(&app, ADMIN, "POST", &format!("{drift}/revert"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["id"], revert["id"]);
    let (_, state) = json_call(
        &app,
        ADMIN,
        "GET",
        &format!("/api/hosts/{host}/state"),
        None,
    )
    .await;
    assert_eq!(state["drift"]["revert_change"], revert["id"]);
    // The revert approves the plan that fixes the drift.
    let key = Authenticator::new(1);
    register_passkey(&app, &key, ORIGIN).await;
    let revert_id = revert["id"].as_str().unwrap();
    let (status, options) = json_call(
        &app,
        ADMIN,
        "POST",
        &format!("/api/changes/{revert_id}/approval"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{options}");
    assert_eq!(options["hosts"][0]["plan_hash"], "d1");

    let (status, draft) = json_call(
        &app,
        ADMIN,
        "POST",
        &format!("{drift}/adopt"),
        Some(json!({ "package": "sl", "role": null })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    assert_eq!(draft["status"], "draft");
    assert_ne!(draft["id"], revert["id"]);
    let (_, state) = json_call(
        &app,
        ADMIN,
        "GET",
        &format!("/api/hosts/{host}/state"),
        None,
    )
    .await;
    assert_eq!(
        state["proposed_adjustments"],
        json!([{ "action": "set", "kind": "package", "name": "sl", "version": "latest" }]),
        "{state}"
    );

    let (status, _) = json_call(&app, ADMIN, "POST", &format!("{drift}/ignore"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, state) = json_call(
        &app,
        ADMIN,
        "GET",
        &format!("/api/hosts/{host}/state"),
        None,
    )
    .await;
    assert_eq!(state["drift"]["ignored"], true);
    assert_eq!(state["undeclared"]["packages"], json!([]));
    assert_eq!(state["undeclared"]["ignored_count"], 2);
    let kinds = events(&pool, &host).await;
    for kind in ["drift.reverted", "drift.adopted", "drift.ignored"] {
        assert!(kinds.contains(&kind.to_owned()), "{kind} in {kinds:?}");
    }
}
