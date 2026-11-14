//! What the agent does with the controller's configs: plans each new one,
//! plans the current desired state again once a day to find drift, and
//! queues the plans for the controller.

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

use crate::opamp::proto::{
    AgentRemoteConfig, AgentToServer, ComponentHealth, CustomMessage, RemoteConfigStatus,
    RemoteConfigStatuses,
};
use crate::opamp::{PLAN_CAPABILITY, PLAN_TYPE};
use crate::plan::{self, CURRENT, Plan, Planner};

/// How often the agent plans the current desired state again.
pub const DRIFT_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct Worker {
    planner: Planner,
    /// The hash of the last remote config, sent back on every message.
    remote_config_hash: Vec<u8>,
    /// Each config and the hash of its body.
    configs: BTreeMap<String, (Vec<u8>, String)>,
    /// The config hash each config was last planned with.
    planned: BTreeMap<String, String>,
    current_planned_at: Option<Instant>,
    health: Option<ComponentHealth>,
    /// Plan reports the controller has not received yet.
    pub queue: VecDeque<Vec<u8>>,
}

impl Worker {
    pub fn new(planner: Planner) -> Self {
        Self {
            planner,
            remote_config_hash: Vec::new(),
            configs: BTreeMap::new(),
            planned: BTreeMap::new(),
            current_planned_at: None,
            health: None,
            queue: VecDeque::new(),
        }
    }

    /// Adds the remote config status, the health, and the next queued plan
    /// to a message. Returns the plan, to queue it again if the message is
    /// lost.
    pub fn fill(&mut self, message: &mut AgentToServer) -> Option<Vec<u8>> {
        if !self.remote_config_hash.is_empty() {
            message.remote_config_status = Some(RemoteConfigStatus {
                last_remote_config_hash: self.remote_config_hash.clone(),
                status: RemoteConfigStatuses::Applied as i32,
                error_message: String::new(),
            });
        }
        message.health = self.health.clone();
        let report = self.queue.pop_front()?;
        message.custom_message = Some(CustomMessage {
            capability: PLAN_CAPABILITY.to_owned(),
            r#type: PLAN_TYPE.to_owned(),
            data: report.clone(),
        });
        Some(report)
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

    fn plan(&mut self, name: &str) {
        let Some((body, hash)) = self.configs.get(name) else {
            return;
        };
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
        let report = plan::report(&plan, explicit.as_ref());
        self.queue.push_back(report);
    }
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
        let mut worker = Worker::new(Planner::new(dir.path(), "mise".into()));
        worker.remote_config_hash = vec![1, 2];
        worker.queue.push_back(b"{}".to_vec());
        let mut message = AgentToServer::default();
        assert_eq!(worker.fill(&mut message), Some(b"{}".to_vec()));
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
}
