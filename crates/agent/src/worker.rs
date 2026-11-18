use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use prost::Message;
use serde::{Deserialize, Serialize};
use sewa_approval::{Outcome, Progress, Signed};
use uuid::Uuid;

use crate::opamp::proto::{
    AgentRemoteConfig, AgentToServer, ComponentHealth, CustomMessage, RemoteConfigStatus,
    RemoteConfigStatuses, ServerToAgent,
};
use crate::opamp::{ExchangeError, PLAN_CAPABILITY, PLAN_TYPE};
use crate::outbox::{self, Outbox};
use crate::plan::{self, CURRENT, Plan, Planner};
use crate::state::State;

pub const DRIFT_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

const REMOTE_CONFIG: &str = "remote_config.pb";
const PLANNED: &str = "planned.json";
const ANSWERED: &str = "answered.json";
const APPLYING: &str = "applying.json";
const ANSWERED_LIMIT: usize = 256;
const PROGRESS_EVERY: Duration = Duration::from_secs(5);
const PROGRESS_QUIET: Duration = Duration::from_secs(20);

#[derive(Debug, Serialize, Deserialize)]
struct Applying {
    approval_id: Uuid,
    change_id: Uuid,
    plan_hash: Option<String>,
}

pub struct Worker {
    planner: Planner,
    state: State,
    instance_uid: Uuid,
    answered: Vec<Uuid>,
    remote_config_hash: Vec<u8>,
    configs: BTreeMap<String, (Vec<u8>, String)>,
    planned: BTreeMap<String, String>,
    current_planned_at: Option<Instant>,
    health: Option<ComponentHealth>,
    pub outbox: Outbox,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

impl Worker {
    pub fn new(planner: Planner, state: State, instance_uid: Uuid) -> Self {
        let outbox = Outbox::open(state.dir.join("outbox"), outbox::LIMIT);
        let answered = state
            .read(ANSWERED)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let planned = state
            .read(PLANNED)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut worker = Self {
            planner,
            state,
            instance_uid,
            answered,
            remote_config_hash: Vec::new(),
            configs: BTreeMap::new(),
            planned,
            current_planned_at: None,
            health: None,
            outbox,
        };
        if let Some(remote) = worker
            .state
            .read(REMOTE_CONFIG)
            .and_then(|bytes| AgentRemoteConfig::decode(&bytes[..]).ok())
        {
            worker.take(remote);
        }
        worker
    }

    pub fn boot(&mut self) {
        if let Some(applying) = self
            .state
            .read(APPLYING)
            .and_then(|bytes| serde_json::from_slice::<Applying>(&bytes).ok())
        {
            eprintln!(
                "sewa-agent: the agent stopped while it applied change {}",
                applying.change_id
            );
            let persisted = self.report_outcome(Outcome {
                approval_id: applying.approval_id,
                change_id: applying.change_id,
                status: "failed".to_owned(),
                plan_hash: applying.plan_hash,
                log: "the agent stopped while mise applied the change, so the result is \
                      unknown; the next plan shows the state of the host"
                    .to_owned(),
                at: Some(now_ms()),
            });
            if persisted {
                self.state.remove(APPLYING);
            }
        }
        self.plan_changed();
        if self.current_planned_at.is_none() {
            self.plan(CURRENT);
        }
    }

    pub fn exchange(
        &mut self,
        mut message: AgentToServer,
        mut send: impl FnMut(&AgentToServer) -> Result<ServerToAgent, ExchangeError>,
    ) -> Result<(), ExchangeError> {
        self.fill(&mut message);
        let answer = send(&message)?;
        if message.custom_message.take().is_some() {
            self.outbox.pop();
        }
        if let Some(remote) = answer.remote_config {
            self.receive(remote);
        }
        if let Some(custom) = &answer.custom_message {
            self.approval(custom, &mut |progress| {
                let mut message = message.clone();
                message.custom_message = Some(progress);
                let _ = send(&message);
            });
        }
        Ok(())
    }

    pub fn fill(&self, message: &mut AgentToServer) {
        if !self.remote_config_hash.is_empty() {
            message.remote_config_status = Some(RemoteConfigStatus {
                last_remote_config_hash: self.remote_config_hash.clone(),
                status: RemoteConfigStatuses::Applied as i32,
                error_message: String::new(),
            });
        }
        message.health = self.health.clone();
        message.custom_message = self.outbox.front().cloned();
    }

    pub fn receive(&mut self, remote: AgentRemoteConfig) {
        if let Err(error) = self.state.write(REMOTE_CONFIG, &remote.encode_to_vec()) {
            eprintln!("sewa-agent: cannot keep the config on disk: {error}");
        }
        self.take(remote);
        let names: Vec<&str> = self.configs.keys().map(String::as_str).collect();
        self.planner.keep_only(&names);
        self.plan_changed();
    }

    fn take(&mut self, remote: AgentRemoteConfig) {
        self.remote_config_hash = remote.config_hash;
        self.configs = remote
            .config
            .map(|map| map.config_map)
            .unwrap_or_default()
            .into_iter()
            .filter(|(name, _)| plan::dir_name(name).is_some())
            .map(|(name, file)| {
                let hash = plan::hash(&file.body);
                (name, (file.body, hash))
            })
            .collect();
        self.planned
            .retain(|name, _| self.configs.contains_key(name));
    }

    fn plan_changed(&mut self) {
        let changed: Vec<String> = self
            .configs
            .iter()
            .filter(|(name, (_, hash))| self.planned.get(*name) != Some(hash))
            .map(|(name, _)| name.clone())
            .collect();
        for name in changed {
            self.plan(&name);
        }
    }

    pub fn tick(&mut self, now: Instant) {
        let due = self
            .current_planned_at
            .is_some_and(|at| now.duration_since(at) >= DRIFT_INTERVAL);
        if due && self.configs.contains_key(CURRENT) {
            self.plan(CURRENT);
        }
    }

    pub fn approval(&mut self, custom: &CustomMessage, progress: &mut dyn FnMut(CustomMessage)) {
        if custom.capability != sewa_approval::CAPABILITY
            || custom.r#type != sewa_approval::APPROVAL_TYPE
        {
            return;
        }
        let signed = match serde_json::from_slice::<Signed>(&custom.data) {
            Ok(signed) => signed,
            Err(error) => {
                eprintln!("sewa-agent: cannot read an approval: {error}");
                return;
            }
        };
        if self.answered.contains(&signed.approval.approval_id) {
            return;
        }
        self.answered.push(signed.approval.approval_id);
        if self.answered.len() > ANSWERED_LIMIT {
            self.answered.remove(0);
        }
        let answered = serde_json::to_vec(&self.answered).unwrap_or_default();
        if let Err(error) = self.state.write(ANSWERED, &answered) {
            eprintln!("sewa-agent: cannot keep the answered approvals on disk: {error}");
        }
        let (status, plan_hash, log) = self.check_and_apply(&signed, progress);
        eprintln!(
            "sewa-agent: approval {} of change {}: {status}",
            signed.approval.approval_id, signed.approval.change_id
        );
        let outcome = Outcome {
            approval_id: signed.approval.approval_id,
            change_id: signed.approval.change_id,
            status: status.to_owned(),
            plan_hash,
            log,
            at: Some(now_ms()),
        };
        let replan = status == "applied" || status == "failed" && plan_hash_known(&outcome);
        if self.report_outcome(outcome) {
            self.state.remove(APPLYING);
        }
        if replan {
            self.plan(CURRENT);
        }
    }

    fn report_outcome(&mut self, outcome: Outcome) -> bool {
        self.outbox.push(CustomMessage {
            capability: sewa_approval::CAPABILITY.to_owned(),
            r#type: sewa_approval::RESULT_TYPE.to_owned(),
            data: serde_json::to_vec(&outcome).unwrap_or_default(),
        })
    }

    fn check_and_apply(
        &mut self,
        signed: &Signed,
        progress: &mut dyn FnMut(CustomMessage),
    ) -> (&'static str, Option<String>, String) {
        let refused = |why: String| ("failed", None, format!("refused: {why}"));
        let pinned = self.state.approvers();
        if let Err(rejection) =
            sewa_approval::verify(&pinned, &signed.assertion, &signed.approval.challenge())
        {
            return refused(format!("the approval does not verify: {rejection}"));
        }
        let Some(entry) = signed.approval.host(self.instance_uid) else {
            return refused("the approval does not cover this agent".to_owned());
        };
        let Some((_, config_hash)) = self.configs.get(CURRENT) else {
            return refused("this agent has no current config".to_owned());
        };
        if *config_hash != entry.config_hash {
            return (
                "replan",
                None,
                "the current config is not the approved one".to_owned(),
            );
        }
        let plan = self.plan(CURRENT).expect("current is known");
        if plan.exit_code != 0 && plan.exit_code != 2 {
            return (
                "failed",
                Some(plan.plan_hash),
                format!(
                    "refused: the plan failed: {}",
                    plan.error.unwrap_or_default()
                ),
            );
        }
        if plan.plan_hash != entry.plan_hash {
            return (
                "replan",
                Some(plan.plan_hash),
                "the plan changed since the approval".to_owned(),
            );
        }
        let applying = Applying {
            approval_id: signed.approval.approval_id,
            change_id: signed.approval.change_id,
            plan_hash: Some(plan.plan_hash.clone()),
        };
        if let Err(error) = self
            .state
            .write(APPLYING, &serde_json::to_vec(&applying).unwrap_or_default())
        {
            return refused(format!("cannot write down the apply first: {error}"));
        }
        let mut reporter = Reporter::new(&signed.approval, progress);
        let (exit_code, log) = self.planner.apply(CURRENT, &mut |step, log| {
            reporter.report(step, log, Instant::now())
        });
        let status = if exit_code == 0 { "applied" } else { "failed" };
        (status, Some(plan.plan_hash), log)
    }

    fn plan(&mut self, name: &str) -> Option<Plan> {
        let (body, hash) = self.configs.get(name)?;
        let plan = self.planner.plan(name, body);
        self.planned.insert(name.to_owned(), hash.clone());
        if let Err(error) = self.state.write(
            PLANNED,
            &serde_json::to_vec(&self.planned).unwrap_or_default(),
        ) {
            eprintln!("sewa-agent: cannot keep the planned configs on disk: {error}");
        }
        let explicit = (name == CURRENT).then(plan::explicit_packages).flatten();
        if name == CURRENT {
            self.current_planned_at = Some(Instant::now());
            self.health = Some(health(&plan));
        }
        eprintln!(
            "sewa-agent: planned {name}: exit {}{}",
            plan.exit_code,
            plan.error
                .as_deref()
                .map(|error| format!(", {error}"))
                .unwrap_or_default()
        );
        self.outbox.push(CustomMessage {
            capability: PLAN_CAPABILITY.to_owned(),
            r#type: PLAN_TYPE.to_owned(),
            data: plan::report(&plan, explicit.as_ref(), now_ms()),
        });
        Some(plan)
    }
}

struct Reporter<'a> {
    approval_id: Uuid,
    change_id: Uuid,
    started_at: i64,
    step: String,
    log: String,
    sent_at: Option<Instant>,
    send: &'a mut dyn FnMut(CustomMessage),
}

impl<'a> Reporter<'a> {
    fn new(approval: &sewa_approval::Approval, send: &'a mut dyn FnMut(CustomMessage)) -> Self {
        Self {
            approval_id: approval.approval_id,
            change_id: approval.change_id,
            started_at: now_ms(),
            step: String::new(),
            log: String::new(),
            sent_at: None,
            send,
        }
    }

    fn due(&self, step: &str, log: &str, now: Instant) -> bool {
        let Some(sent_at) = self.sent_at else {
            return true;
        };
        let since = now.saturating_duration_since(sent_at);
        step != self.step || since >= PROGRESS_QUIET || since >= PROGRESS_EVERY && log != self.log
    }

    fn report(&mut self, step: &str, log: &str, now: Instant) {
        if !self.due(step, log, now) {
            return;
        }
        step.clone_into(&mut self.step);
        log.clone_into(&mut self.log);
        self.sent_at = Some(now);
        let progress = Progress {
            approval_id: self.approval_id,
            change_id: self.change_id,
            step: step.to_owned(),
            started_at: self.started_at,
            at: now_ms(),
            log: plan::limit(log.to_owned()),
        };
        (self.send)(CustomMessage {
            capability: sewa_approval::CAPABILITY.to_owned(),
            r#type: sewa_approval::PROGRESS_TYPE.to_owned(),
            data: serde_json::to_vec(&progress).unwrap_or_default(),
        });
    }
}

fn plan_hash_known(outcome: &Outcome) -> bool {
    outcome.plan_hash.is_some() && !outcome.log.starts_with("refused: ")
}

pub fn health(plan: &Plan) -> ComponentHealth {
    let status = match plan.exit_code {
        0 => "in_sync",
        2 => "drift",
        _ => "error",
    };
    ComponentHealth {
        healthy: plan.exit_code == 0,
        status: status.to_owned(),
        last_error: plan.error.clone().unwrap_or_default(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(exit_code: i32) -> Plan {
        Plan {
            config: CURRENT.to_owned(),
            config_hash: String::new(),
            plan_hash: String::new(),
            exit_code,
            plan: None,
            error: None,
        }
    }

    #[test]
    fn health_follows_the_exit_code() {
        assert_eq!(health(&plan(0)).status, "in_sync");
        assert!(health(&plan(0)).healthy);
        assert_eq!(health(&plan(2)).status, "drift");
        assert!(!health(&plan(2)).healthy);
        assert_eq!(health(&plan(1)).status, "error");
    }

    #[test]
    fn a_queued_plan_goes_out_with_the_config_hash() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = Worker::new(
            Planner::new(dir.path(), "mise".into()),
            State::new(dir.path().to_owned()),
            Uuid::now_v7(),
        );
        worker.remote_config_hash = vec![1, 2];
        let queued = CustomMessage {
            capability: PLAN_CAPABILITY.to_owned(),
            r#type: PLAN_TYPE.to_owned(),
            data: b"{}".to_vec(),
        };
        worker.outbox.push(queued.clone());
        let mut message = AgentToServer::default();
        worker.fill(&mut message);
        assert_eq!(
            message
                .remote_config_status
                .unwrap()
                .last_remote_config_hash,
            vec![1, 2]
        );
        assert_eq!(message.custom_message, Some(queued));
        assert_eq!(worker.outbox.len(), 1);
    }

    mod approvals {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        use sewa_approval::testing::Authenticator;
        use sewa_approval::{Approval, Assertion, HostPlan, Pinned};

        use super::*;
        use crate::native::{NO_HOMEBREW, Native};
        use crate::opamp::proto::{AgentConfigMap, AgentConfigObject};

        const RP_ID: &str = "sewa.example.com";
        const ORIGIN: &str = "https://sewa.example.com";
        const BODY: &[u8] = b"[tools]\nripgrep = \"latest\"\n";

        struct Fixture {
            _root: tempfile::TempDir,
            root: PathBuf,
            worker: Worker,
            body: Vec<u8>,
            uid: Uuid,
            host: Uuid,
            key: Authenticator,
            progress: Vec<Progress>,
        }

        fn fake_mise(root: &Path) -> PathBuf {
            let path = root.join("mise");
            let script = format!(
                r#"#!/bin/sh
for argument in "$@"; do
  case "$argument" in
    plan) cat '{plan}'; exit 2 ;;
    --yes) echo "$*" >> '{applied}'; echo "would apply"; exit "$(cat '{exit}' 2>/dev/null || echo 0)" ;;
  esac
done
exit 1
"#,
                plan = root.join("plan.json").display(),
                applied = root.join("applied").display(),
                exit = root.join("exit").display(),
            );
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn planner(state: &State, root: &Path) -> Planner {
            let mut planner = Planner::new(&state.dir, root.join("mise"));
            planner.mise_fallbacks.clear();
            planner.native = Native {
                macos: false,
                brew: vec![root.join("no-brew")],
                pacman: vec![root.join("no-pacman")],
            };
            planner
        }

        impl Fixture {
            fn new() -> Self {
                Self::with(BODY, |_, planner| planner)
            }

            fn with(body: &[u8], setup: impl FnOnce(&Path, Planner) -> Planner) -> Self {
                let dir = tempfile::tempdir().unwrap();
                let root = dir.path().to_owned();
                fs::write(
                    root.join("plan.json"),
                    r#"{"packages":[{"name":"ripgrep"}]}"#,
                )
                .unwrap();
                let state = State::new(root.join("state"));
                let key = Authenticator::new(1);
                state
                    .store_approvers(&Pinned {
                        rp_id: RP_ID.to_owned(),
                        origin: ORIGIN.to_owned(),
                        keys: vec![key.approver("admin@example.com")],
                    })
                    .unwrap();
                let uid = Uuid::now_v7();
                fake_mise(&root);
                let planner = setup(&root, planner(&state, &root));
                let mut worker = Worker::new(planner, state, uid);
                let mut config_map = std::collections::HashMap::new();
                config_map.insert(
                    CURRENT.to_owned(),
                    AgentConfigObject {
                        body: body.to_vec(),
                        content_type: "application/toml".to_owned(),
                        ..Default::default()
                    },
                );
                worker.receive(AgentRemoteConfig {
                    config: Some(AgentConfigMap { config_map }),
                    config_hash: vec![1],
                });
                worker.outbox.clear();
                Self {
                    _root: dir,
                    root,
                    worker,
                    body: body.to_vec(),
                    uid,
                    host: Uuid::now_v7(),
                    key,
                    progress: Vec::new(),
                }
            }

            fn approval(&self) -> Approval {
                let plan = fs::read(self.root.join("plan.json")).unwrap();
                Approval {
                    approval_id: Uuid::now_v7(),
                    change_id: Uuid::now_v7(),
                    hosts: vec![HostPlan {
                        host_id: self.host,
                        instance_uid: self.uid,
                        config_hash: plan::hash(&self.body),
                        plan_hash: plan::hash(&plan),
                    }],
                }
            }

            fn sign(&self, approval: &Approval) -> Assertion {
                self.key.assert(RP_ID, ORIGIN, &approval.challenge())
            }

            fn send(&mut self, approval: Approval, assertion: Assertion) -> Outcome {
                let signed = Signed {
                    approval,
                    assertion,
                };
                self.worker.outbox.clear();
                let mut progress = Vec::new();
                self.worker.approval(
                    &CustomMessage {
                        capability: sewa_approval::CAPABILITY.to_owned(),
                        r#type: sewa_approval::APPROVAL_TYPE.to_owned(),
                        data: serde_json::to_vec(&signed).unwrap(),
                    },
                    &mut |message| progress.push(message),
                );
                self.progress = progress
                    .iter()
                    .map(|message| {
                        assert_eq!(message.r#type, sewa_approval::PROGRESS_TYPE);
                        serde_json::from_slice(&message.data).unwrap()
                    })
                    .collect();
                let outcome = self
                    .worker
                    .outbox
                    .iter()
                    .find(|message| message.capability == sewa_approval::CAPABILITY)
                    .expect("a result");
                assert_eq!(outcome.r#type, sewa_approval::RESULT_TYPE);
                let outcome: Outcome = serde_json::from_slice(&outcome.data).unwrap();
                assert_eq!(outcome.approval_id, signed.approval.approval_id);
                assert_eq!(outcome.change_id, signed.approval.change_id);
                outcome
            }

            fn applies(&self) -> Vec<String> {
                fs::read_to_string(self.root.join("applied"))
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_owned)
                    .collect()
            }

            fn plans_queued(&self) -> usize {
                self.worker
                    .outbox
                    .iter()
                    .filter(|message| message.capability == PLAN_CAPABILITY)
                    .count()
            }

            fn assert_refused(&mut self, approval: Approval, assertion: Assertion, why: &str) {
                let outcome = self.send(approval, assertion);
                assert_eq!(outcome.status, "failed", "{outcome:?}");
                assert!(outcome.log.contains(why), "{why} in {:?}", outcome.log);
                assert!(self.applies().is_empty(), "nothing is applied");
                assert!(self.progress.is_empty(), "{:?}", self.progress);
            }

            fn restart(&mut self) {
                let state = State::new(self.root.join("state"));
                let planner = planner(&state, &self.root);
                self.worker = Worker::new(planner, state, self.uid);
                self.worker.boot();
            }

            fn signed(&self) -> CustomMessage {
                let approval = self.approval();
                let signed = Signed {
                    assertion: self.sign(&approval),
                    approval,
                };
                CustomMessage {
                    capability: sewa_approval::CAPABILITY.to_owned(),
                    r#type: sewa_approval::APPROVAL_TYPE.to_owned(),
                    data: serde_json::to_vec(&signed).unwrap(),
                }
            }

            fn offline(&mut self, times: usize) {
                for _ in 0..times {
                    let sent = self
                        .worker
                        .exchange(AgentToServer::default(), |_| Err(ExchangeError::Network));
                    assert_eq!(sent, Err(ExchangeError::Network));
                }
            }

            fn online(&mut self, answer: &ServerToAgent) -> Vec<CustomMessage> {
                let mut received = Vec::new();
                for _ in 0..20 {
                    self.worker
                        .exchange(AgentToServer::default(), |message| {
                            received.extend(message.custom_message.clone());
                            Ok(answer.clone())
                        })
                        .unwrap();
                    if self.worker.outbox.is_empty() {
                        return received;
                    }
                }
                panic!("the outbox does not empty");
            }
        }

        fn at(message: &CustomMessage) -> i64 {
            let value: serde_json::Value = serde_json::from_slice(&message.data).unwrap();
            value["at"].as_i64().expect("a time")
        }

        fn outcomes(received: &[CustomMessage]) -> Vec<Outcome> {
            received
                .iter()
                .filter(|message| message.capability == sewa_approval::CAPABILITY)
                .map(|message| serde_json::from_slice(&message.data).unwrap())
                .collect()
        }

        fn drifted(root: &Path) {
            fs::write(
                root.join("plan.json"),
                r#"{"packages":[{"name":"ripgrep"},{"name":"htop","action":"remove"}]}"#,
            )
            .unwrap();
        }

        #[test]
        fn controller_down_at_start_plans_the_last_config_and_buffers() {
            let mut fixture = Fixture::new();
            drifted(&fixture.root);
            fixture.restart();
            assert_eq!(fixture.plans_queued(), 1, "the boot plan of current");
            fixture.offline(3);
            assert_eq!(fixture.worker.outbox.len(), 1);
            assert!(fixture.applies().is_empty());
            let planned_at = at(fixture.worker.outbox.front().unwrap());
            let mut message = AgentToServer::default();
            fixture.worker.fill(&mut message);
            assert_eq!(message.health.unwrap().status, "drift");
            assert_eq!(
                message
                    .remote_config_status
                    .unwrap()
                    .last_remote_config_hash,
                vec![1]
            );
            std::thread::sleep(Duration::from_millis(5));
            let received = fixture.online(&ServerToAgent::default());
            assert_eq!(received.len(), 1);
            let report: serde_json::Value = serde_json::from_slice(&received[0].data).unwrap();
            assert_eq!(report["exit_code"], 2);
            assert_eq!(at(&received[0]), planned_at);
            assert!(planned_at < now_ms());
        }

        #[test]
        fn controller_down_mid_run_keeps_checking_and_applies_nothing() {
            let mut fixture = Fixture::new();
            let signed = fixture.signed();
            assert!(fixture.online(&ServerToAgent::default()).is_empty());
            fixture.offline(2);
            let later = Instant::now() + DRIFT_INTERVAL;
            fixture.worker.tick(later);
            drifted(&fixture.root);
            fixture.worker.tick(later + DRIFT_INTERVAL);
            fixture.offline(2);
            assert_eq!(fixture.plans_queued(), 2);
            assert!(fixture.applies().is_empty());
            let received = fixture.online(&ServerToAgent::default());
            let hashes: Vec<String> = received
                .iter()
                .map(|message| {
                    let report: serde_json::Value = serde_json::from_slice(&message.data).unwrap();
                    report["plan_hash"].as_str().unwrap().to_owned()
                })
                .collect();
            assert_eq!(hashes.len(), 2);
            assert_ne!(hashes[0], hashes[1]);
            assert!(at(&received[0]) <= at(&received[1]));
            assert!(fixture.applies().is_empty());
            let answer = ServerToAgent {
                custom_message: Some(signed),
                ..Default::default()
            };
            let received = fixture.online(&answer);
            assert_eq!(outcomes(&received)[0].status, "replan");
            assert!(fixture.applies().is_empty());
        }

        #[test]
        fn buffered_reports_replay_in_order_after_a_restart() {
            let mut fixture = Fixture::new();
            let answer = ServerToAgent {
                custom_message: Some(fixture.signed()),
                ..Default::default()
            };
            fixture
                .worker
                .exchange(AgentToServer::default(), |_| Ok(answer.clone()))
                .unwrap();
            fixture.offline(1);
            fixture.worker.tick(Instant::now() + DRIFT_INTERVAL);
            let before: Vec<CustomMessage> = fixture.worker.outbox.iter().cloned().collect();
            let kinds: Vec<&str> = before.iter().map(|m| m.capability.as_str()).collect();
            assert_eq!(
                kinds,
                [
                    PLAN_CAPABILITY,
                    sewa_approval::CAPABILITY,
                    PLAN_CAPABILITY,
                    PLAN_CAPABILITY
                ]
            );
            fixture.restart();
            let received = fixture.online(&ServerToAgent::default());
            assert_eq!(received.len(), 5);
            assert_eq!(received[..4], before[..]);
            let times: Vec<i64> = received.iter().map(at).collect();
            assert!(times.is_sorted(), "{times:?}");
        }

        #[test]
        fn a_restart_between_apply_and_report_keeps_the_result() {
            let mut fixture = Fixture::new();
            let answer = ServerToAgent {
                custom_message: Some(fixture.signed()),
                ..Default::default()
            };
            fixture
                .worker
                .exchange(AgentToServer::default(), |_| Ok(answer.clone()))
                .unwrap();
            assert_eq!(fixture.applies().len(), 1);
            fixture.restart();
            let received = fixture.online(&answer);
            assert_eq!(fixture.applies().len(), 1, "applied once");
            let outcomes = outcomes(&received);
            assert_eq!(outcomes.len(), 1);
            assert_eq!(outcomes[0].status, "applied");
            assert!(outcomes[0].log.contains("would apply"));
        }

        #[test]
        fn a_restart_during_the_apply_reports_an_unknown_result() {
            let mut fixture = Fixture::new();
            let signed = fixture.signed();
            let approval: Signed = serde_json::from_slice(&signed.data).unwrap();
            let state = &fixture.worker.state;
            state
                .write(
                    ANSWERED,
                    &serde_json::to_vec(&[approval.approval.approval_id]).unwrap(),
                )
                .unwrap();
            let applying = Applying {
                approval_id: approval.approval.approval_id,
                change_id: approval.approval.change_id,
                plan_hash: Some("p".to_owned()),
            };
            state
                .write(APPLYING, &serde_json::to_vec(&applying).unwrap())
                .unwrap();
            fixture.restart();
            let answer = ServerToAgent {
                custom_message: Some(signed),
                ..Default::default()
            };
            let received = fixture.online(&answer);
            assert!(fixture.applies().is_empty(), "not applied again");
            let outcomes = outcomes(&received);
            assert_eq!(outcomes.len(), 1);
            assert_eq!(outcomes[0].status, "failed");
            assert!(outcomes[0].log.contains("stopped"), "{:?}", outcomes[0].log);
            assert!(fixture.worker.state.read(APPLYING).is_none());
        }

        #[test]
        fn a_signed_approval_of_the_plan_applies_it() {
            let mut fixture = Fixture::new();
            let approval = fixture.approval();
            let assertion = fixture.sign(&approval);
            let plan_hash = approval.hosts[0].plan_hash.clone();
            let outcome = fixture.send(approval, assertion);
            assert_eq!(outcome.status, "applied", "{outcome:?}");
            assert_eq!(outcome.plan_hash, Some(plan_hash));
            assert!(outcome.log.contains("would apply"));
            let applies = fixture.applies();
            assert_eq!(applies.len(), 1);
            assert!(
                applies[0].ends_with("bootstrap --yes --only packages,tools"),
                "{applies:?}"
            );
            assert_eq!(fixture.plans_queued(), 2);
        }

        #[test]
        fn an_apply_reports_its_step_while_it_runs() {
            let mut fixture = Fixture::new();
            let approval = fixture.approval();
            let assertion = fixture.sign(&approval);
            let started = now_ms();
            let outcome = fixture.send(approval.clone(), assertion);
            assert_eq!(outcome.status, "applied");
            let progress = &fixture.progress[0];
            assert_eq!(progress.step, plan::MISE_STEP);
            assert_eq!(progress.approval_id, approval.approval_id);
            assert_eq!(progress.change_id, approval.change_id);
            assert!(progress.started_at >= started && progress.at >= progress.started_at);
        }

        #[test]
        fn progress_goes_out_before_the_result_and_its_answer_is_ignored() {
            let mut fixture = Fixture::new();
            let answer = ServerToAgent {
                custom_message: Some(fixture.signed()),
                ..Default::default()
            };
            let mut other = std::collections::HashMap::new();
            other.insert(
                CURRENT.to_owned(),
                AgentConfigObject {
                    body: b"[tools]\nfd = \"latest\"\n".to_vec(),
                    ..Default::default()
                },
            );
            let ignored = ServerToAgent {
                remote_config: Some(AgentRemoteConfig {
                    config: Some(AgentConfigMap { config_map: other }),
                    config_hash: vec![9],
                }),
                custom_message: Some(fixture.signed()),
                ..Default::default()
            };
            let mut sent = Vec::new();
            fixture
                .worker
                .exchange(AgentToServer::default(), |message| {
                    sent.push(message.custom_message.clone());
                    Ok(if sent.len() == 1 {
                        answer.clone()
                    } else {
                        ignored.clone()
                    })
                })
                .unwrap();
            assert_eq!(sent.len(), 2, "{sent:?}");
            let progress = sent[1].as_ref().unwrap();
            assert_eq!(progress.r#type, sewa_approval::PROGRESS_TYPE);
            assert_eq!(fixture.applies().len(), 1, "the second approval is ignored");
            assert_eq!(fixture.worker.remote_config_hash, vec![1]);
            let kinds: Vec<&str> = fixture
                .worker
                .outbox
                .iter()
                .map(|message| message.r#type.as_str())
                .collect();
            assert_eq!(kinds, [PLAN_TYPE, sewa_approval::RESULT_TYPE, PLAN_TYPE]);
        }

        #[test]
        fn progress_follows_steps_and_waits_between_lines() {
            let approval = Approval {
                approval_id: Uuid::now_v7(),
                change_id: Uuid::now_v7(),
                hosts: Vec::new(),
            };
            let mut sent = Vec::new();
            let mut send = |message: CustomMessage| {
                sent.push(serde_json::from_slice::<Progress>(&message.data).unwrap());
            };
            let mut reporter = Reporter::new(&approval, &mut send);
            let start = Instant::now();
            let after = |seconds| start + Duration::from_secs(seconds);
            reporter.report("native", "", start);
            reporter.report("native", "a\n", after(1));
            reporter.report("mise", "a\n", after(2));
            reporter.report("mise", "a\nb\n", after(3));
            reporter.report("mise", "a\nb\nc\n", after(7));
            reporter.report("mise", "a\nb\nc\n", after(20));
            reporter.report("mise", "a\nb\nc\n", after(27));
            let seen: Vec<(String, String)> = sent
                .iter()
                .map(|progress| (progress.step.clone(), progress.log.clone()))
                .collect();
            assert_eq!(
                seen,
                [
                    ("native", ""),
                    ("mise", "a\n"),
                    ("mise", "a\nb\nc\n"),
                    ("mise", "a\nb\nc\n"),
                ]
                .map(|(step, log)| (step.to_owned(), log.to_owned()))
            );
        }

        #[test]
        fn a_failed_apply_reports_the_log() {
            let mut fixture = Fixture::new();
            fs::write(fixture.root.join("exit"), "1").unwrap();
            let approval = fixture.approval();
            let assertion = fixture.sign(&approval);
            let outcome = fixture.send(approval, assertion);
            assert_eq!(outcome.status, "failed");
            assert!(outcome.log.contains("would apply"));
            assert_eq!(fixture.applies().len(), 1);
        }

        #[test]
        fn an_approval_is_answered_once() {
            let mut fixture = Fixture::new();
            let approval = fixture.approval();
            let signed = Signed {
                assertion: fixture.sign(&approval),
                approval,
            };
            let custom = CustomMessage {
                capability: sewa_approval::CAPABILITY.to_owned(),
                r#type: sewa_approval::APPROVAL_TYPE.to_owned(),
                data: serde_json::to_vec(&signed).unwrap(),
            };
            fixture.worker.approval(&custom, &mut |_| {});
            fixture.worker.approval(&custom, &mut |_| {});
            assert_eq!(fixture.applies().len(), 1);
        }

        #[test]
        fn a_replayed_assertion_under_a_new_id_applies_nothing() {
            let mut fixture = Fixture::new();
            let approval = fixture.approval();
            let assertion = fixture.sign(&approval);
            let outcome = fixture.send(approval.clone(), assertion.clone());
            assert_eq!(outcome.status, "applied", "{outcome:?}");
            let mut approval = approval;
            approval.approval_id = Uuid::now_v7();
            let replay = Signed {
                approval,
                assertion,
            };
            fixture.worker.approval(
                &CustomMessage {
                    capability: sewa_approval::CAPABILITY.to_owned(),
                    r#type: sewa_approval::APPROVAL_TYPE.to_owned(),
                    data: serde_json::to_vec(&replay).unwrap(),
                },
                &mut |_| {},
            );
            assert_eq!(fixture.applies().len(), 1, "the replay applies nothing");
        }

        #[test]
        fn a_forged_assertion_applies_nothing() {
            let mut fixture = Fixture::new();
            let approval = fixture.approval();
            let mut forger = Authenticator::new(2);
            forger.id = fixture.key.id.clone();
            let assertion = forger.assert(RP_ID, ORIGIN, &approval.challenge());
            fixture.assert_refused(approval, assertion, "the signature does not verify");
            let approval = fixture.approval();
            let assertion = Authenticator::new(3).assert(RP_ID, ORIGIN, &approval.challenge());
            fixture.assert_refused(approval, assertion, "not a pinned approver key");
            let approval = fixture.approval();
            let assertion = fixture.key.assert(
                "evil.example.com",
                "https://evil.example.com",
                &approval.challenge(),
            );
            fixture.assert_refused(approval, assertion, "does not match");
        }

        #[test]
        fn a_missing_assertion_or_key_applies_nothing() {
            let mut fixture = Fixture::new();
            let approval = fixture.approval();
            let empty = Assertion {
                credential_id: String::new(),
                authenticator_data: String::new(),
                client_data_json: String::new(),
                signature: String::new(),
            };
            fixture.assert_refused(approval, empty, "not a pinned approver key");
            fixture
                .worker
                .state
                .store_approvers(&Pinned::default())
                .unwrap();
            let approval = fixture.approval();
            let assertion = fixture.sign(&approval);
            fixture.assert_refused(approval, assertion, "no approver key is pinned");
        }

        #[test]
        fn an_assertion_of_another_plan_applies_nothing() {
            let mut fixture = Fixture::new();
            let mut approval = fixture.approval();
            approval.hosts[0].plan_hash = "0".repeat(64);
            let signed_for = fixture.approval();
            let assertion = fixture.sign(&signed_for);
            let mut tampered = signed_for.clone();
            tampered.hosts[0].plan_hash = "0".repeat(64);
            fixture.assert_refused(tampered, assertion, "the challenge does not match");
            let assertion = fixture.sign(&approval);
            let outcome = fixture.send(approval, assertion);
            assert_eq!(outcome.status, "replan");
            assert!(fixture.applies().is_empty());
        }

        #[test]
        fn an_assertion_of_another_change_applies_nothing() {
            let mut fixture = Fixture::new();
            let approval = fixture.approval();
            let assertion = fixture.sign(&approval);
            let mut other = approval.clone();
            other.change_id = Uuid::now_v7();
            fixture.assert_refused(other, assertion, "the challenge does not match");
        }

        #[test]
        fn an_approval_of_another_agent_applies_nothing() {
            let mut fixture = Fixture::new();
            let mut approval = fixture.approval();
            approval.hosts[0].instance_uid = Uuid::now_v7();
            let assertion = fixture.sign(&approval);
            fixture.assert_refused(approval, assertion, "does not cover this agent");
        }

        #[test]
        fn a_plan_that_changed_since_the_approval_replans_without_applying() {
            let mut fixture = Fixture::new();
            let approval = fixture.approval();
            let assertion = fixture.sign(&approval);
            fs::write(
                fixture.root.join("plan.json"),
                r#"{"packages":[{"name":"ripgrep"},{"name":"fd"}]}"#,
            )
            .unwrap();
            let fresh = plan::hash(&fs::read(fixture.root.join("plan.json")).unwrap());
            let outcome = fixture.send(approval, assertion);
            assert_eq!(outcome.status, "replan");
            assert_eq!(outcome.plan_hash, Some(fresh.clone()));
            assert!(fixture.applies().is_empty());
            let report = fixture
                .worker
                .outbox
                .iter()
                .find(|message| message.capability == PLAN_CAPABILITY)
                .expect("a plan report");
            let report: serde_json::Value = serde_json::from_slice(&report.data).unwrap();
            assert_eq!(report["plan_hash"], fresh);
        }
        fn mac(root: &Path, mut planner: Planner) -> Planner {
            let brew = root.join("brew");
            let script = format!(
                r#"#!/bin/sh
echo "$*" >> '{log}'
case "$1" in
  list) cat '{installed}' 2>/dev/null; exit 0 ;;
  install)
    shift
    for name in "$@"; do
      echo "$name" >> '{installed}'
      [ "$name" = mise ] && cp '{fake}' '{mise}'
    done
    exit 0 ;;
esac
exit 1
"#,
                log = root.join("brew.log").display(),
                installed = root.join("installed").display(),
                fake = root.join("mise").display(),
                mise = root.join("bin/mise").display(),
            );
            fs::write(&brew, script).unwrap();
            fs::set_permissions(&brew, fs::Permissions::from_mode(0o755)).unwrap();
            fs::create_dir_all(root.join("bin")).unwrap();
            planner.mise = root.join("bin/mise");
            planner.native = Native {
                macos: true,
                brew: vec![brew],
                pacman: vec![],
            };
            planner
        }

        const MAC_BODY: &[u8] = b"[bootstrap.packages]\n\"brew:mise\" = \"latest\"\n\
            \"brew:wget\" = \"latest\"\n\n[tools]\nripgrep = \"latest\"\n";

        impl Fixture {
            fn approval_of_a_fresh_plan(&mut self) -> Approval {
                let plan = self.worker.plan(CURRENT).unwrap();
                let mut approval = self.approval();
                approval.hosts[0].plan_hash = plan.plan_hash;
                approval
            }

            fn brew_log(&self) -> Vec<String> {
                fs::read_to_string(self.root.join("brew.log"))
                    .unwrap_or_default()
                    .lines()
                    .map(str::to_owned)
                    .collect()
            }
        }

        fn actions(plan: &Plan) -> Vec<(String, String)> {
            plan.plan.as_ref().unwrap()["resources"]
                .as_array()
                .unwrap()
                .iter()
                .map(|resource| {
                    (
                        resource["id"]["name"].as_str().unwrap().to_owned(),
                        resource["action"].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        }

        #[test]
        fn a_mac_without_mise_gets_mise_from_brew_then_the_mise_items() {
            let mut fixture = Fixture::with(MAC_BODY, mac);
            let plan = fixture.worker.plan(CURRENT).unwrap();
            assert_eq!(plan.exit_code, 2, "{plan:?}");
            assert_eq!(plan.error, None);
            assert_eq!(
                actions(&plan),
                [
                    ("brew:mise", "create"),
                    ("brew:wget", "create"),
                    ("ripgrep", "create"),
                ]
                .map(|(name, action)| (name.to_owned(), action.to_owned()))
            );
            let approval = fixture.approval_of_a_fresh_plan();
            let assertion = fixture.sign(&approval);
            let outcome = fixture.send(approval, assertion);
            assert_eq!(outcome.status, "applied", "{outcome:?}");
            let brew = fixture.brew_log();
            assert!(brew.contains(&"install mise wget".to_owned()), "{brew:?}");
            assert_eq!(fixture.applies().len(), 1, "{:?}", outcome.log);
            assert!(outcome.log.contains("install mise wget"), "{}", outcome.log);
            assert!(outcome.log.contains("would apply"), "{}", outcome.log);
            let plan = fixture.worker.plan(CURRENT).unwrap();
            assert_eq!(
                actions(&plan)[0],
                ("brew:mise".to_owned(), "noop".to_owned())
            );
        }

        #[test]
        fn a_mac_without_mise_applies_brew_items_and_fails_only_the_mise_items() {
            let body = b"[bootstrap.packages]\n\"brew:wget\" = \"latest\"\n\n[tools]\nripgrep = \"latest\"\n";
            let mut fixture = Fixture::with(body, mac);
            let plan = fixture.worker.plan(CURRENT).unwrap();
            assert_eq!(plan.exit_code, 2, "{plan:?}");
            assert!(
                plan.error.as_deref().unwrap().contains("mise is missing"),
                "{plan:?}"
            );
            assert_eq!(
                actions(&plan),
                [("brew:wget", "create"), ("ripgrep", "unknown")]
                    .map(|(name, action)| (name.to_owned(), action.to_owned()))
            );
            let approval = fixture.approval_of_a_fresh_plan();
            let assertion = fixture.sign(&approval);
            let outcome = fixture.send(approval, assertion);
            assert_eq!(outcome.status, "failed", "{outcome:?}");
            assert!(fixture.brew_log().contains(&"install wget".to_owned()));
            assert!(fixture.applies().is_empty());
            assert!(
                outcome
                    .log
                    .contains("mise is missing; its items were not applied"),
                "{}",
                outcome.log
            );
        }

        #[test]
        fn a_mac_without_homebrew_plans_nothing_on_top() {
            let mut fixture = Fixture::with(MAC_BODY, |root, planner| {
                let mut planner = mac(root, planner);
                planner.native.brew = vec![root.join("no-brew")];
                planner
            });
            let plan = fixture.worker.plan(CURRENT).unwrap();
            assert_eq!(plan.exit_code, 1);
            assert_eq!(plan.error.as_deref(), Some(NO_HOMEBREW));
            assert_eq!(
                actions(&plan),
                [("brew:mise", "unknown"), ("brew:wget", "unknown")]
                    .map(|(name, action)| (name.to_owned(), action.to_owned()))
            );
            assert_eq!(health(&plan).status, "error");
            assert!(fixture.applies().is_empty());
        }
    }
}
