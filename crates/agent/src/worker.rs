//! What the agent does with the controller's configs: plans each new one,
//! plans the current desired state again once a day to find drift, applies
//! `current` when a pinned approver key signed its plan, and queues the
//! plans and apply results for the controller.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use sewa_approval::{Outcome, Signed};
use uuid::Uuid;

use crate::opamp::proto::{
    AgentRemoteConfig, AgentToServer, ComponentHealth, CustomMessage, RemoteConfigStatus,
    RemoteConfigStatuses,
};
use crate::opamp::{PLAN_CAPABILITY, PLAN_TYPE};
use crate::plan::{self, CURRENT, Plan, Planner};
use crate::state::State;

/// How often the agent plans the current desired state again.
pub const DRIFT_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct Worker {
    planner: Planner,
    /// Where the pinned approver keys are.
    state: State,
    instance_uid: Uuid,
    /// The approvals this run already answered.
    answered: HashSet<Uuid>,
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
    pub queue: VecDeque<CustomMessage>,
}

impl Worker {
    pub fn new(planner: Planner, state: State, instance_uid: Uuid) -> Self {
        Self {
            planner,
            state,
            instance_uid,
            answered: HashSet::new(),
            remote_config_hash: Vec::new(),
            configs: BTreeMap::new(),
            planned: BTreeMap::new(),
            current_planned_at: None,
            health: None,
            queue: VecDeque::new(),
        }
    }

    /// Adds the remote config status, the health, and the next queued
    /// message to a message. Returns the queued message, to queue it again
    /// if the message is lost.
    pub fn fill(&mut self, message: &mut AgentToServer) -> Option<CustomMessage> {
        if !self.remote_config_hash.is_empty() {
            message.remote_config_status = Some(RemoteConfigStatus {
                last_remote_config_hash: self.remote_config_hash.clone(),
                status: RemoteConfigStatuses::Applied as i32,
                error_message: String::new(),
            });
        }
        message.health = self.health.clone();
        let queued = self.queue.pop_front()?;
        message.custom_message = Some(queued.clone());
        Some(queued)
    }

    /// Takes a new remote config: drops configs the controller no longer
    /// sends, and plans the ones that changed.
    pub fn receive(&mut self, remote: AgentRemoteConfig) {
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
        let names: Vec<&str> = self.configs.keys().map(String::as_str).collect();
        self.planner.keep_only(&names);
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
    /// is answered once.
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
        if !self.answered.insert(signed.approval_id) {
            return;
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
        };
        self.queue.push_back(CustomMessage {
            capability: sewa_approval::CAPABILITY.to_owned(),
            r#type: sewa_approval::RESULT_TYPE.to_owned(),
            data: serde_json::to_vec(&outcome).unwrap_or_default(),
        });
        if status == "applied" || status == "failed" && plan_hash_known(&outcome) {
            self.plan(CURRENT);
        }
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
        let (exit_code, log) = self.planner.apply(CURRENT);
        let status = if exit_code == 0 { "applied" } else { "failed" };
        (status, Some(plan.plan_hash), log)
    }

    fn plan(&mut self, name: &str) -> Option<Plan> {
        let (body, hash) = self.configs.get(name)?;
        let plan = self.planner.plan(name, body);
        self.planned.insert(name.to_owned(), hash.clone());
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
        self.queue.push_back(CustomMessage {
            capability: PLAN_CAPABILITY.to_owned(),
            r#type: PLAN_TYPE.to_owned(),
            data: plan::report(&plan, explicit.as_ref()),
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
        worker.queue.push_back(queued.clone());
        let mut message = AgentToServer::default();
        assert_eq!(worker.fill(&mut message), Some(queued));
        assert_eq!(
            message
                .remote_config_status
                .unwrap()
                .last_remote_config_hash,
            vec![1, 2]
        );
        let custom = message.custom_message.unwrap();
        assert_eq!(custom.capability, PLAN_CAPABILITY);
        assert_eq!(custom.data, b"{}");
        assert!(worker.queue.is_empty());
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
                worker.queue.clear();
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
                self.worker.queue.clear();
                self.worker.approval(&CustomMessage {
                    capability: sewa_approval::CAPABILITY.to_owned(),
                    r#type: sewa_approval::APPROVAL_TYPE.to_owned(),
                    data: serde_json::to_vec(&signed).unwrap(),
                });
                let outcome = self
                    .worker
                    .queue
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
                    .queue
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
                .queue
                .iter()
                .find(|message| message.capability == PLAN_CAPABILITY)
                .expect("a plan report");
            let report: serde_json::Value = serde_json::from_slice(&report.data).unwrap();
            assert_eq!(report["plan_hash"], fresh);
        }
    }
}
