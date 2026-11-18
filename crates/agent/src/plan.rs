use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::native::{self, Native, Split};

pub const CURRENT: &str = "current";

const ERROR_LIMIT: usize = 4000;

const LOG_LIMIT: usize = 32 * 1024;

const SPLIT: &str = "split.json";

const TICK: Duration = Duration::from_secs(1);

pub const NATIVE_STEP: &str = "native";

pub const MISE_STEP: &str = "mise";

const MISE_MISSING: &str = "mise is missing";

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn hash(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn dir_name(config: &str) -> Option<String> {
    if config == CURRENT {
        return Some(CURRENT.to_owned());
    }
    let id = config.strip_prefix("change/")?;
    let id = uuid::Uuid::try_parse(id).ok()?;
    Some(format!("change-{}", id.hyphenated()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub config: String,
    pub config_hash: String,
    pub plan_hash: String,
    pub exit_code: i32,
    pub plan: Option<Value>,
    pub error: Option<String>,
}

pub struct Planner {
    pub mise: PathBuf,
    pub mise_fallbacks: Vec<PathBuf>,
    pub native: Native,
    pub configs: PathBuf,
    pub empty: PathBuf,
}

impl Planner {
    pub fn new(state_dir: &Path, mise: PathBuf) -> Self {
        Self {
            mise,
            mise_fallbacks: vec![
                "/opt/homebrew/bin/mise".into(),
                "/usr/local/bin/mise".into(),
            ],
            native: Native::this(),
            configs: state_dir.join("configs"),
            empty: state_dir.join("mise-empty"),
        }
    }

    pub fn dir(&self, config: &str) -> Option<PathBuf> {
        Some(self.configs.join(dir_name(config)?))
    }

    pub fn command(
        &self,
        dir: &Path,
        inherited: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Command {
        let mut command = Command::new(self.find_mise().unwrap_or_else(|| self.mise.clone()));
        command.env_clear();
        command.envs(
            inherited
                .into_iter()
                .filter(|(key, _)| !key.to_string_lossy().starts_with("MISE_")),
        );
        command
            .env("MISE_SYSTEM_CONFIG_DIR", &self.empty)
            .env("MISE_CONFIG_DIR", &self.empty)
            .env("MISE_GLOBAL_CONFIG_FILE", self.empty.join("config.toml"))
            .env("MISE_CEILING_PATHS", &self.configs)
            .env("MISE_TRUSTED_CONFIG_PATHS", &self.configs)
            .arg("-C")
            .arg(dir)
            .stdin(Stdio::null());
        command
    }

    pub fn find_mise(&self) -> Option<PathBuf> {
        let mut candidates = vec![self.mise.clone()];
        if self.mise.components().count() == 1 {
            candidates.extend(self.mise_fallbacks.iter().cloned());
        }
        native::find(&candidates)
    }

    pub fn plan(&self, config: &str, body: &[u8]) -> Plan {
        let config_hash = hash(body);
        let failed = |error: String| Plan {
            config: config.to_owned(),
            config_hash: config_hash.clone(),
            plan_hash: hash(b""),
            exit_code: 1,
            plan: None,
            error: Some(error),
        };
        let Some(dir) = self.dir(config) else {
            return failed(format!("unknown config name {config}"));
        };
        let split = self.native.split(body);
        if let Err(error) = self.write(&dir, body, &split) {
            return failed(format!("cannot write {}: {error}", dir.display()));
        }
        let mut natives = Vec::new();
        if !split.native.is_empty() {
            match self.native.plan(&split.native) {
                Ok(resources) => natives = resources,
                Err(error) => {
                    let plan = json!({
                        "resources": split
                            .native
                            .iter()
                            .map(|item| native::resource(&item.key, "unknown"))
                            .collect::<Vec<_>>(),
                    });
                    return Plan {
                        config: config.to_owned(),
                        config_hash,
                        plan_hash: hash(&serde_json::to_vec(&plan).unwrap_or_default()),
                        exit_code: 1,
                        plan: Some(plan),
                        error: Some(error),
                    };
                }
            }
        }
        let (exit_code, plan, raw, error) = self.plan_mise(&dir, &split, &natives);
        if split.native.is_empty() {
            return Plan {
                config: config.to_owned(),
                config_hash,
                plan_hash: hash(&raw),
                exit_code: if plan.is_none() { 1 } else { exit_code },
                plan,
                error,
            };
        }
        let changes = natives.iter().any(|resource| resource["action"] != "noop");
        let mut plan = plan.unwrap_or_else(|| json!({ "resources": [] }));
        if !plan["resources"].is_array() {
            plan["resources"] = json!([]);
        }
        if let Some(resources) = plan["resources"].as_array_mut() {
            resources.splice(0..0, natives);
        }
        let exit_code = match (exit_code, changes) {
            (_, true) => 2,
            (0 | 2, false) => exit_code,
            _ => 1,
        };
        Plan {
            config: config.to_owned(),
            config_hash,
            plan_hash: hash(&serde_json::to_vec(&plan).unwrap_or_default()),
            exit_code,
            plan: Some(plan),
            error,
        }
    }

    fn plan_mise(
        &self,
        dir: &Path,
        split: &Split,
        natives: &[Value],
    ) -> (i32, Option<Value>, Vec<u8>, Option<String>) {
        let Some(mise) = self.find_mise() else {
            let installs_mise = split
                .native
                .iter()
                .zip(natives)
                .any(|(item, resource)| item.name == "mise" && resource["action"] == "create");
            let (exit_code, action, error) = if split.rest.is_empty() {
                (0, "noop", None)
            } else if installs_mise {
                (2, "create", None)
            } else {
                (
                    1,
                    "unknown",
                    Some(format!("{MISE_MISSING}; its items cannot be planned")),
                )
            };
            let plan = json!({
                "resources": split
                    .rest
                    .iter()
                    .map(|(kind, name)| json!({ "id": { "kind": kind, "name": name }, "action": action }))
                    .collect::<Vec<_>>(),
            });
            let raw = serde_json::to_vec(&plan).unwrap_or_default();
            return (exit_code, Some(plan), raw, error);
        };
        let output = match self
            .command(dir, std::env::vars_os())
            .args(["bootstrap", "plan", "--json", "--detailed-exitcode"])
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                return (
                    1,
                    None,
                    Vec::new(),
                    Some(format!("cannot run {}: {error}", mise.display())),
                );
            }
        };
        let exit_code = output.status.code().unwrap_or(1);
        let plan = serde_json::from_slice::<Value>(&output.stdout).ok();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let error = (exit_code != 0 && exit_code != 2 || plan.is_none())
            .then(|| truncate(stderr.trim()))
            .filter(|error| !error.is_empty());
        (exit_code, plan, output.stdout, error)
    }

    pub fn apply(&self, config: &str, progress: &mut dyn FnMut(&str, &str)) -> (i32, String) {
        let Some(dir) = self.dir(config) else {
            return (1, format!("unknown config name {config}"));
        };
        let split: Split = fs::read(dir.join(SPLIT))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut log = String::new();
        let mut exit_code = 0;
        if !split.native.is_empty() {
            progress(NATIVE_STEP, &log);
            let (code, native_log) = thread::scope(|scope| {
                let native = scope.spawn(|| self.native.apply(&split.native));
                while !native.is_finished() {
                    thread::sleep(TICK);
                    progress(NATIVE_STEP, &log);
                }
                native
                    .join()
                    .unwrap_or_else(|_| (1, "the native apply panicked\n".to_owned()))
            });
            log.push_str(&native_log);
            if code != 0 {
                exit_code = code;
            }
            if code != 0 && native_log.contains(native::NO_HOMEBREW) {
                return (exit_code, limit(log));
            }
        }
        if split.native.is_empty() || !split.rest.is_empty() {
            progress(MISE_STEP, &log);
            let Some(mise) = self.find_mise() else {
                log.push_str(&format!("{MISE_MISSING}; its items were not applied\n"));
                return (1, limit(log));
            };
            let mut command = self.command(&dir, std::env::vars_os());
            command.args(["bootstrap", "--yes", "--only", "packages,tools"]);
            match stream(command, &mut log, &mut |live| progress(MISE_STEP, live)) {
                Ok(code) => {
                    if code != 0 {
                        exit_code = code;
                    }
                }
                Err(error) => {
                    log.push_str(&format!("cannot run {}: {error}\n", mise.display()));
                    exit_code = 1;
                }
            }
        }
        (exit_code, limit(log))
    }

    fn write(&self, dir: &Path, body: &[u8], split: &Split) -> io::Result<()> {
        let mut builder = DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(&self.empty)?;
        builder.create(dir)?;
        let mise = split.mise.as_deref().map_or(body, str::as_bytes);
        crate::state::write_private(&dir.join("mise.toml"), mise)?;
        crate::state::write_private(
            &dir.join(SPLIT),
            &serde_json::to_vec(split).unwrap_or_default(),
        )
    }

    pub fn keep_only(&self, configs: &[&str]) {
        let keep: Vec<String> = configs.iter().filter_map(|c| dir_name(c)).collect();
        let Ok(entries) = fs::read_dir(&self.configs) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !keep.contains(&name) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

fn stream(
    mut command: Command,
    log: &mut String,
    progress: &mut dyn FnMut(&str),
) -> io::Result<i32> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let (sender, lines) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        read_lines(stdout, false, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        read_lines(stderr, true, sender);
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut live = log.clone();
    loop {
        match lines.recv_timeout(TICK) {
            Ok((is_stderr, line)) => {
                live.push_str(&String::from_utf8_lossy(&line));
                if live.len() > 2 * LOG_LIMIT {
                    live = limit(live);
                }
                if is_stderr {
                    stderr.extend(line);
                } else {
                    stdout.extend(line);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        progress(&live);
    }
    let status = child.wait()?;
    log.push_str(&String::from_utf8_lossy(&stdout));
    log.push_str(&String::from_utf8_lossy(&stderr));
    Ok(status.code().unwrap_or(1))
}

fn read_lines(pipe: impl Read + Send + 'static, is_stderr: bool, sender: Sender<(bool, Vec<u8>)>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        loop {
            let mut line = Vec::new();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if sender.send((is_stderr, line)).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

pub fn limit(mut log: String) -> String {
    if log.len() > LOG_LIMIT {
        let mut start = log.len() - LOG_LIMIT;
        while !log.is_char_boundary(start) {
            start += 1;
        }
        log = format!("…{}", &log[start..]);
    }
    log
}

fn truncate(text: &str) -> String {
    if text.len() <= ERROR_LIMIT {
        return text.to_owned();
    }
    let mut end = ERROR_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

pub fn explicit_packages() -> Option<(&'static str, Vec<String>)> {
    let candidates: &[(&str, &str, &[&str])] = if cfg!(target_os = "macos") {
        &[("brew", "brew", &["leaves"])]
    } else {
        &[
            ("pacman", "pacman", &["-Qqe"]),
            ("apt", "apt-mark", &["showmanual"]),
        ]
    };
    candidates.iter().find_map(|(manager, program, args)| {
        let output = Command::new(program)
            .args(*args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        output.status.success().then(|| {
            let mut packages: Vec<String> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect();
            packages.sort();
            packages.dedup();
            (*manager, packages)
        })
    })
}

pub fn report(plan: &Plan, explicit: Option<&(&str, Vec<String>)>, at: i64) -> Vec<u8> {
    let mut value = json!({
        "at": at,
        "config": plan.config,
        "config_hash": plan.config_hash,
        "plan_hash": plan.plan_hash,
        "exit_code": plan.exit_code,
        "plan": plan.plan,
        "error": plan.error,
    });
    if let Some((manager, packages)) = explicit {
        value["explicit"] = json!({ "manager": manager, "packages": packages });
    }
    value.to_string().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_names_map_to_directories() {
        assert_eq!(dir_name("current").as_deref(), Some("current"));
        let id = uuid::Uuid::nil();
        assert_eq!(
            dir_name(&format!("change/{id}")).as_deref(),
            Some("change-00000000-0000-0000-0000-000000000000")
        );
        assert_eq!(dir_name("change/../etc"), None);
        assert_eq!(dir_name("other"), None);
    }

    #[test]
    fn the_command_drops_inherited_mise_variables() {
        let planner = Planner::new(Path::new("/state"), PathBuf::from("mise"));
        let command = planner.command(
            Path::new("/state/configs/current"),
            [
                ("PATH".into(), "/usr/bin".into()),
                ("MISE_ENV".into(), "leak".into()),
            ],
        );
        let envs: Vec<(String, String)> = command
            .get_envs()
            .filter_map(|(key, value)| {
                Some((
                    key.to_string_lossy().into_owned(),
                    value?.to_string_lossy().into_owned(),
                ))
            })
            .collect();
        assert!(envs.contains(&("PATH".to_owned(), "/usr/bin".to_owned())));
        assert!(!envs.iter().any(|(key, _)| key == "MISE_ENV"));
        assert!(envs.contains(&(
            "MISE_SYSTEM_CONFIG_DIR".to_owned(),
            "/state/mise-empty".to_owned()
        )));
        assert!(envs.contains(&("MISE_CEILING_PATHS".to_owned(), "/state/configs".to_owned())));
    }

    #[test]
    fn mise_reads_only_the_config() {
        if Command::new("mise").arg("--version").output().is_err() {
            eprintln!("mise is not installed; skipped");
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let leak = "[bootstrap.packages]\n\"sewa-leak-test\" = \"latest\"\n";
        let leaks = root.path().join("leaks");
        fs::create_dir_all(leaks.join("conf.d")).unwrap();
        fs::write(leaks.join("config.toml"), leak).unwrap();
        fs::write(leaks.join("conf.d/leak.toml"), leak).unwrap();
        fs::create_dir_all(state.join("configs")).unwrap();
        fs::write(root.path().join("mise.toml"), leak).unwrap();
        fs::write(state.join("mise.toml"), leak).unwrap();
        fs::write(state.join("configs/mise.toml"), leak).unwrap();
        let planner = Planner::new(&state, PathBuf::from("mise"));
        let config = "# empty\n";
        let plan = planner.plan(CURRENT, config.as_bytes());
        assert_eq!(plan.config_hash, hash(config.as_bytes()));

        let dir = planner.dir(CURRENT).unwrap();
        let mut inherited: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        inherited.push(("MISE_SYSTEM_CONFIG_DIR".into(), leaks.clone().into()));
        inherited.push(("MISE_CONFIG_DIR".into(), leaks.clone().into()));
        inherited.push((
            "MISE_GLOBAL_CONFIG_FILE".into(),
            leaks.join("config.toml").into(),
        ));
        let output = planner
            .command(&dir, inherited.clone())
            .args(["config", "ls", "--no-header"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let listed: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
            .collect();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert!(
            listed[0].ends_with("configs/current/mise.toml"),
            "{listed:?}"
        );

        let output = planner
            .command(&dir, inherited)
            .args(["bootstrap", "plan", "--json", "--detailed-exitcode"])
            .output()
            .unwrap();
        let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(plan["resources"], json!([]), "{plan}");
        assert_eq!(output.status.code(), Some(0));
    }

    #[test]
    fn the_apply_streams_mise_output_while_it_runs() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let mise = root.path().join("mise");
        fs::write(
            &mise,
            "#!/bin/sh\necho out\necho one >&2\nsleep 2\necho two >&2\nexit 3\n",
        )
        .unwrap();
        fs::set_permissions(&mise, fs::Permissions::from_mode(0o755)).unwrap();
        let planner = Planner::new(&root.path().join("state"), mise);
        fs::create_dir_all(planner.dir(CURRENT).unwrap()).unwrap();
        let mut seen: Vec<(String, String)> = Vec::new();
        let (code, log) = planner.apply(CURRENT, &mut |step, live| {
            seen.push((step.to_owned(), live.to_owned()));
        });
        assert_eq!(code, 3);
        assert_eq!(log, "out\none\ntwo\n");
        assert_eq!(seen[0], (MISE_STEP.to_owned(), String::new()));
        assert!(
            seen.iter()
                .any(|(_, live)| live.contains("one") && !live.contains("two")),
            "{seen:?}"
        );
    }

    #[test]
    fn a_slow_native_apply_keeps_reporting_its_step() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let pacman = root.path().join("pacman");
        fs::write(
            &pacman,
            "#!/bin/sh\n[ \"$1\" = -Qq ] && exit 0\nsleep 3\necho done\n",
        )
        .unwrap();
        fs::set_permissions(&pacman, fs::Permissions::from_mode(0o755)).unwrap();
        let mut planner = Planner::new(&root.path().join("state"), root.path().join("mise"));
        planner.native = Native {
            macos: false,
            brew: Vec::new(),
            pacman: vec![pacman],
        };
        let dir = planner.dir(CURRENT).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let split = Split {
            native: vec![native::Item {
                key: "pacman:htop".to_owned(),
                manager: native::Manager::Pacman,
                name: "htop".to_owned(),
                absent: false,
            }],
            mise: None,
            rest: Vec::new(),
        };
        fs::write(dir.join(SPLIT), serde_json::to_vec(&split).unwrap()).unwrap();
        let mut steps = Vec::new();
        let (code, log) = planner.apply(CURRENT, &mut |step, _| steps.push(step.to_owned()));
        assert_eq!(code, 0, "{log}");
        assert!(log.ends_with("done\n"), "{log}");
        assert!(steps.len() >= 3, "{steps:?}");
        assert!(steps.iter().all(|step| step == NATIVE_STEP), "{steps:?}");
    }

    #[test]
    fn a_report_carries_the_plan_and_the_explicit_packages() {
        let plan = Plan {
            config: CURRENT.to_owned(),
            config_hash: "c".to_owned(),
            plan_hash: "p".to_owned(),
            exit_code: 2,
            plan: Some(json!({ "resources": [] })),
            error: None,
        };
        let explicit = ("pacman", vec!["git".to_owned()]);
        let value: Value = serde_json::from_slice(&report(&plan, Some(&explicit), 7)).unwrap();
        assert_eq!(value["exit_code"], 2);
        assert_eq!(value["at"], 7);
        assert_eq!(
            value["explicit"],
            json!({ "manager": "pacman", "packages": ["git"] })
        );
    }
}
