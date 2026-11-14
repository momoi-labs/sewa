//! What the agent does with the controller's configs: plans each new one,
//! plans the current desired state again on start and once a day to find
//! drift, applies `current` when a pinned approver key signed its plan, and
//! keeps the plans and apply results in the outbox for the controller.
//!
//! The worker keeps what it needs on disk, so it works on when the
//! controller is down and after a restart: the last remote config, the
//! config hash each config was planned with, the approvals it answered, and
//! the outbox. While the controller is down it applies nothing: an apply
//! needs an approval the controller sends.

use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use prost::Message;
use serde::{Deserialize, Serialize};
use sewa_approval::{Outcome, Signed};
use uuid::Uuid;

use crate::opamp::proto::{
    AgentRemoteConfig, AgentToServer, ComponentHealth, CustomMessage, RemoteConfigStatus,
    RemoteConfigStatuses, ServerToAgent,
};
use crate::opamp::{ExchangeError, PLAN_CAPABILITY, PLAN_TYPE};
use crate::outbox::{self, Outbox};
use crate::plan::{self, CURRENT, Plan, Planner};
use crate::state::State;

/// How often the agent plans the current desired state again.
pub const DRIFT_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// The last remote config, as OpAMP encodes it.
const REMOTE_CONFIG: &str = "remote_config.pb";
/// The config hash each config was last planned with.
const PLANNED: &str = "planned.json";
/// The approvals the agent answered, newest last.
const ANSWERED: &str = "answered.json";
/// The approval being applied. It exists only while mise applies.
const APPLYING: &str = "applying.json";
/// How many answered approvals the agent remembers.
const ANSWERED_LIMIT: usize = 256;

/// What the agent writes down before it applies, so a restart in the
/// middle still reports.
#[derive(Debug, Serialize, Deserialize)]
struct Applying {
    approval_id: Uuid,
    change_id: Uuid,
    plan_hash: Option<String>,
}

pub struct Worker {
    planner: Planner,
    /// Where the pinned approver keys and the worker's files are.
    state: State,
    instance_uid: Uuid,
    /// The approvals the agent answered, newest last.
    answered: Vec<Uuid>,
    /// The hash of the last remote config, sent back on every message.
    remote_config_hash: Vec<u8>,
    /// Each config and the hash of its body.
    configs: BTreeMap<String, (Vec<u8>, String)>,
    /// The config hash each config was last planned with.
    planned: BTreeMap<String, String>,
    current_planned_at: Option<Instant>,
    health: Option<ComponentHealth>,
    /// Plan reports and apply results the controller has not received
    /// yet.
    pub outbox: Outbox,
}

/// Now, in Unix milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

impl Worker {
    /// A worker with what an earlier run left in the state directory.
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

    /// What the agent does when it starts, before it reaches the
    /// controller: reports an apply that a restart cut short, plans the
    /// configs it did not plan yet, and plans the approved state again.
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

    /// Sends one message with `send`, the exchange with the controller.
    /// The message carries the remote config status, the health, and the
    /// oldest report of the outbox, which leaves the outbox only when the
    /// controller answers. Then the worker takes the answer's config and
    /// approval.
    pub fn exchange(
        &mut self,
        mut message: AgentToServer,
        send: impl FnOnce(&AgentToServer) -> Result<ServerToAgent, ExchangeError>,
    ) -> Result<(), ExchangeError> {
        self.fill(&mut message);
        let answer = send(&message)?;
        if message.custom_message.is_some() {
            self.outbox.pop();
        }
        if let Some(remote) = answer.remote_config {
            self.receive(remote);
        }
        // After the config of the same answer, which the approval may be
        // about.
        if let Some(custom) = &answer.custom_message {
            self.approval(custom);
        }
        Ok(())
    }

    /// Adds the remote config status, the health, and the oldest report of
    /// the outbox to a message.
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

    /// Takes a new remote config: keeps it on disk, drops configs the
    /// controller no longer sends, and plans the ones that changed.
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

    /// Plans each config whose body changed since its last plan.
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

    /// Plans the current desired state again when a day has passed.
    pub fn tick(&mut self, now: Instant) {
        let due = self
            .current_planned_at
            .is_some_and(|at| now.duration_since(at) >= DRIFT_INTERVAL);
        if due && self.configs.contains_key(CURRENT) {
            self.plan(CURRENT);
        }
    }

    /// Takes an approval from the controller. Applies `current` only when
    /// a pinned approver key signed this change and this agent's plan, the
    /// config is the one approved, and a fresh plan still has the approved
    /// hash. Queues the result, and the plan after an apply. Each approval
    /// is answered once, across restarts.
    pub fn approval(&mut self, custom: &CustomMessage) {
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
        if self.answered.contains(&signed.approval_id) {
            return;
        }
        self.answered.push(signed.approval_id);
        if self.answered.len() > ANSWERED_LIMIT {
            self.answered.remove(0);
        }
        let answered = serde_json::to_vec(&self.answered).unwrap_or_default();
        if let Err(error) = self.state.write(ANSWERED, &answered) {
            eprintln!("sewa-agent: cannot keep the answered approvals on disk: {error}");
        }
        let (status, plan_hash, log) = self.check_and_apply(&signed);
        eprintln!(
            "sewa-agent: approval {} of change {}: {status}",
            signed.approval_id, signed.approval.change_id
        );
        let outcome = Outcome {
            approval_id: signed.approval_id,
            change_id: signed.approval.change_id,
            status: status.to_owned(),
            plan_hash,
            log,
            at: Some(now_ms()),
        };
        let replan = status == "applied" || status == "failed" && plan_hash_known(&outcome);
        // The marker goes only once the result is on disk.
        if self.report_outcome(outcome) {
            self.state.remove(APPLYING);
        }
        if replan {
            self.plan(CURRENT);
        }
    }

    /// Puts a result in the outbox. Returns whether it reached the disk.
    fn report_outcome(&mut self, outcome: Outcome) -> bool {
        self.outbox.push(CustomMessage {
            capability: sewa_approval::CAPABILITY.to_owned(),
            r#type: sewa_approval::RESULT_TYPE.to_owned(),
            data: serde_json::to_vec(&outcome).unwrap_or_default(),
        })
    }

    /// `applied`, `failed`, or `replan`, the fresh plan hash, and the log.
    fn check_and_apply(&mut self, signed: &Signed) -> (&'static str, Option<String>, String) {
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
            approval_id: signed.approval_id,
            change_id: signed.approval.change_id,
            plan_hash: Some(plan.plan_hash.clone()),
        };
        if let Err(error) = self
            .state
            .write(APPLYING, &serde_json::to_vec(&applying).unwrap_or_default())
        {
            return refused(format!("cannot write down the apply first: {error}"));
        }
        let (exit_code, log) = self.planner.apply(CURRENT);
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

/// Whether a failed outcome got as far as mise, so the state may have
/// changed and needs a new plan.
fn plan_hash_known(outcome: &Outcome) -> bool {
    outcome.plan_hash.is_some() && !outcome.log.starts_with("refused: ")
}

/// `in_sync` when the plan has no changes, `drift` when it has, `error`
/// when mise failed or found a state it cannot read.
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
        // It leaves the outbox only once the controller answers.
        assert_eq!(worker.outbox.len(), 1);
    }

    mod approvals {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::{Path, PathBuf};

        use sewa_approval::testing::Authenticator;
        use sewa_approval::{Approval, Assertion, HostPlan, Pinned};

        use super::*;
        use crate::opamp::proto::{AgentConfigMap, AgentConfigObject};

        const RP_ID: &str = "sewa.example.com";
        const ORIGIN: &str = "https://sewa.example.com";
        const BODY: &[u8] = b"[bootstrap.packages]\n\"pacman:ripgrep\" = \"latest\"\n";

        /// An agent whose mise is a fake: it prints `plan.json` as the
        /// plan, and only writes down an apply in `applied`. Nothing on
        /// this machine is ever applied.
        struct Fixture {
            _root: tempfile::TempDir,
            root: PathBuf,
            worker: Worker,
            uid: Uuid,
            host: Uuid,
            key: Authenticator,
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

        impl Fixture {
            fn new() -> Self {
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
                let planner = Planner::new(&state.dir, fake_mise(&root));
                let mut worker = Worker::new(planner, state, uid);
                let mut config_map = std::collections::HashMap::new();
                config_map.insert(
                    CURRENT.to_owned(),
                    AgentConfigObject {
                        body: BODY.to_vec(),
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
                    uid,
                    host: Uuid::now_v7(),
                    key,
                }
            }

            /// The approval of the plan as it is now.
            fn approval(&self) -> Approval {
                let plan = fs::read(self.root.join("plan.json")).unwrap();
                Approval {
                    change_id: Uuid::now_v7(),
                    hosts: vec![HostPlan {
                        host_id: self.host,
                        instance_uid: self.uid,
                        config_hash: plan::hash(BODY),
                        plan_hash: plan::hash(&plan),
                    }],
                }
            }

            fn sign(&self, approval: &Approval) -> Assertion {
                self.key.assert(RP_ID, ORIGIN, &approval.challenge())
            }

            /// Sends an approval and returns the result.
            fn send(&mut self, approval: Approval, assertion: Assertion) -> Outcome {
                let signed = Signed {
                    approval_id: Uuid::now_v7(),
                    approval,
                    assertion,
                };
                self.worker.outbox.clear();
                self.worker.approval(&CustomMessage {
                    capability: sewa_approval::CAPABILITY.to_owned(),
                    r#type: sewa_approval::APPROVAL_TYPE.to_owned(),
                    data: serde_json::to_vec(&signed).unwrap(),
                });
                let outcome = self
                    .worker
                    .outbox
                    .iter()
                    .find(|message| message.capability == sewa_approval::CAPABILITY)
                    .expect("a result");
                assert_eq!(outcome.r#type, sewa_approval::RESULT_TYPE);
                let outcome: Outcome = serde_json::from_slice(&outcome.data).unwrap();
                assert_eq!(outcome.approval_id, signed.approval_id);
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
            }

            /// The agent stops and starts again on the same state
            /// directory, as after a crash or a reboot.
            fn restart(&mut self) {
                let state = State::new(self.root.join("state"));
                let planner = Planner::new(&state.dir, self.root.join("mise"));
                self.worker = Worker::new(planner, state, self.uid);
                self.worker.boot();
            }

            /// The approval message the controller sends, signed.
            fn signed(&self) -> CustomMessage {
                let approval = self.approval();
                let signed = Signed {
                    approval_id: Uuid::now_v7(),
                    assertion: self.sign(&approval),
                    approval,
                };
                CustomMessage {
                    capability: sewa_approval::CAPABILITY.to_owned(),
                    r#type: sewa_approval::APPROVAL_TYPE.to_owned(),
                    data: serde_json::to_vec(&signed).unwrap(),
                }
            }

            /// Messages to a controller that is down.
            fn offline(&mut self, times: usize) {
                for _ in 0..times {
                    let sent = self
                        .worker
                        .exchange(AgentToServer::default(), |_| Err(ExchangeError::Network));
                    assert_eq!(sent, Err(ExchangeError::Network));
                }
            }

            /// Messages to a controller that answers `answer`, until the
            /// outbox is empty. Returns the reports it received.
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
            // The host changes while the agent is stopped, and the
            // controller is down when it starts again.
            drifted(&fixture.root);
            fixture.restart();
            assert_eq!(fixture.plans_queued(), 1, "the boot plan of current");
            fixture.offline(3);
            assert_eq!(fixture.worker.outbox.len(), 1);
            assert!(fixture.applies().is_empty());
            let planned_at = at(fixture.worker.outbox.front().unwrap());
            // Health and the config hash come from the config on disk.
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
            // The controller goes down. The daily checks go on.
            fixture.offline(2);
            let later = Instant::now() + DRIFT_INTERVAL;
            fixture.worker.tick(later);
            drifted(&fixture.root);
            fixture.worker.tick(later + DRIFT_INTERVAL);
            fixture.offline(2);
            assert_eq!(fixture.plans_queued(), 2);
            assert!(fixture.applies().is_empty());
            // It comes back: the reports arrive in order, then the
            // approval it sends now is the first one the agent can act on.
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
            // An approval of the old plan does not apply the drifted one.
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
            // The approval arrives, then the controller goes down.
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
            // The boot plan goes after what the last run left.
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
            // The agent stops before its next message. The controller
            // still sends the approval, since it has no result yet.
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
            // What the agent wrote down before mise ran, then it stopped.
            let state = &fixture.worker.state;
            state
                .write(
                    ANSWERED,
                    &serde_json::to_vec(&[approval.approval_id]).unwrap(),
                )
                .unwrap();
            let applying = Applying {
                approval_id: approval.approval_id,
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
            // The plan before the apply and the plan after it.
            assert_eq!(fixture.plans_queued(), 2);
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
                approval_id: Uuid::now_v7(),
                assertion: fixture.sign(&approval),
                approval,
            };
            let custom = CustomMessage {
                capability: sewa_approval::CAPABILITY.to_owned(),
                r#type: sewa_approval::APPROVAL_TYPE.to_owned(),
                data: serde_json::to_vec(&signed).unwrap(),
            };
            fixture.worker.approval(&custom);
            fixture.worker.approval(&custom);
            assert_eq!(fixture.applies().len(), 1);
        }

        #[test]
        fn a_forged_assertion_applies_nothing() {
            let mut fixture = Fixture::new();
            let approval = fixture.approval();
            // Another key that claims the pinned credential.
            let mut forger = Authenticator::new(2);
            forger.id = fixture.key.id.clone();
            let assertion = forger.assert(RP_ID, ORIGIN, &approval.challenge());
            fixture.assert_refused(approval, assertion, "the signature does not verify");
            // A key that is not pinned.
            let approval = fixture.approval();
            let assertion = Authenticator::new(3).assert(RP_ID, ORIGIN, &approval.challenge());
            fixture.assert_refused(approval, assertion, "not a pinned approver key");
            // A signature by the pinned key for another relying party.
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
            // No key pinned: even a valid signature applies nothing.
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
            // The controller swaps the plan hash after the person signed.
            let mut approval = fixture.approval();
            approval.hosts[0].plan_hash = "0".repeat(64);
            let signed_for = fixture.approval();
            let assertion = fixture.sign(&signed_for);
            let mut tampered = signed_for.clone();
            tampered.hosts[0].plan_hash = "0".repeat(64);
            fixture.assert_refused(tampered, assertion, "the challenge does not match");
            // A signature that is valid, but of another plan, re-plans.
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
            // The machine changes between the approval and the apply.
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
            // The fresh plan goes to the controller for a new approval.
            let report = fixture
                .worker
                .outbox
                .iter()
                .find(|message| message.capability == PLAN_CAPABILITY)
                .expect("a plan report");
            let report: serde_json::Value = serde_json::from_slice(&report.data).unwrap();
            assert_eq!(report["plan_hash"], fresh);
        }
    }
}
