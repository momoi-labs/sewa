//! Packages the agent installs with the OS package manager itself, without
//! mise: `brew:` and `brew-cask:` keys on macOS, `pacman:` keys on Linux,
//! when the version is `latest` or the state is `absent`. The built-in
//! `bootstrap` role puts mise itself here, so a host without mise gets it
//! in the same run that installs the mise items.
//!
//! On macOS a missing Homebrew fails these items, and nothing else is
//! planned on top of them: only the install script installs Homebrew.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// What a host without Homebrew gets for its native items.
pub const NO_HOMEBREW: &str = "Homebrew is missing; rerun the install script";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Manager {
    Brew,
    Cask,
    Pacman,
}

/// One native package of a config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// The key as the config has it, such as `brew:mise`.
    pub key: String,
    pub manager: Manager,
    pub name: String,
    pub absent: bool,
}

/// A config split in two: the native items, and the rest for mise.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Split {
    pub native: Vec<Item>,
    /// The mise config without the native items. `None` when nothing was
    /// taken out: mise reads the body as it came.
    #[serde(skip)]
    pub mise: Option<String>,
    /// The kinds and names of what is left for mise: `package` or `tool`.
    pub rest: Vec<(String, String)>,
}

/// The package managers of this host.
#[derive(Debug, Clone)]
pub struct Native {
    pub macos: bool,
    /// Where to look for `brew`, in order. A bare name is looked up in
    /// `PATH`. launchd's `PATH` has neither Homebrew prefix.
    pub brew: Vec<PathBuf>,
    pub pacman: Vec<PathBuf>,
}

impl Native {
    pub fn this() -> Self {
        Self {
            macos: cfg!(target_os = "macos"),
            brew: vec![
                "/opt/homebrew/bin/brew".into(),
                "/usr/local/bin/brew".into(),
                "brew".into(),
            ],
            pacman: vec!["pacman".into()],
        }
    }

    fn manager(&self, prefix: &str) -> Option<Manager> {
        match (self.macos, prefix) {
            (true, "brew") => Some(Manager::Brew),
            (true, "brew-cask") => Some(Manager::Cask),
            (false, "pacman") => Some(Manager::Pacman),
            _ => None,
        }
    }

    /// Takes the native items out of a config. A body that is not TOML
    /// goes to mise whole, which reports the error.
    pub fn split(&self, body: &[u8]) -> Split {
        let Some(mut table) = std::str::from_utf8(body)
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
        else {
            return Split::default();
        };
        let mut native = Vec::new();
        if let Some(packages) = table
            .get_mut("bootstrap")
            .and_then(|bootstrap| bootstrap.get_mut("packages"))
            .and_then(toml::Value::as_table_mut)
        {
            let keys: Vec<String> = packages.keys().cloned().collect();
            for key in keys {
                let Some((prefix, name)) = key.split_once(':') else {
                    continue;
                };
                let Some(manager) = self.manager(prefix) else {
                    continue;
                };
                let absent = match &packages[&key] {
                    toml::Value::String(version) if version == "latest" => false,
                    toml::Value::Table(spec)
                        if spec.len() == 1
                            && spec.get("state").and_then(toml::Value::as_str)
                                == Some("absent") =>
                    {
                        true
                    }
                    _ => continue,
                };
                packages.remove(&key);
                native.push(Item {
                    key: key.clone(),
                    manager,
                    name: name.to_owned(),
                    absent,
                });
            }
        }
        // Drops tables the split left empty.
        if let Some(bootstrap) = table
            .get_mut("bootstrap")
            .and_then(toml::Value::as_table_mut)
        {
            if bootstrap
                .get("packages")
                .and_then(toml::Value::as_table)
                .is_some_and(toml::Table::is_empty)
            {
                bootstrap.remove("packages");
            }
            if bootstrap.is_empty() {
                table.remove("bootstrap");
            }
        }
        let mut rest = Vec::new();
        let sections = [
            ("package", ["bootstrap", "packages"].as_slice()),
            ("tool", &["tools"]),
        ];
        for (kind, path) in sections {
            let mut value = Some(&table);
            for part in path {
                value = value
                    .and_then(|t| t.get(*part))
                    .and_then(toml::Value::as_table);
            }
            for name in value.into_iter().flat_map(toml::Table::keys) {
                rest.push((kind.to_owned(), name.clone()));
            }
        }
        let mise = (!native.is_empty()).then(|| {
            format!(
                "# The sewa agent installs the native packages of this config itself.\n{}",
                toml::to_string(&table).unwrap_or_default()
            )
        });
        Split { native, mise, rest }
    }

    /// The command of a manager, or why there is none.
    fn program(&self, manager: Manager) -> Result<PathBuf, String> {
        match manager {
            Manager::Brew | Manager::Cask => find(&self.brew).ok_or_else(|| NO_HOMEBREW.to_owned()),
            Manager::Pacman => find(&self.pacman).ok_or_else(|| "pacman is missing".to_owned()),
        }
    }

    /// What each item needs: `create`, `remove`, or `noop`, as resources
    /// of a mise plan.
    pub fn plan(&self, items: &[Item]) -> Result<Vec<Value>, String> {
        let managers: BTreeSet<Manager> = items.iter().map(|item| item.manager).collect();
        let mut installed = Vec::new();
        for manager in managers {
            let program = self.program(manager)?;
            installed.push((manager, list(&program, manager)?));
        }
        Ok(items
            .iter()
            .map(|item| {
                let names = installed
                    .iter()
                    .find(|(manager, _)| *manager == item.manager)
                    .map(|(_, names)| names);
                // `brew list` names a tap formula without its tap.
                let short = item.name.rsplit('/').next().unwrap_or(&item.name);
                let present = names.is_some_and(|names| names.contains(short));
                let action = match (item.absent, present) {
                    (false, false) => "create",
                    (true, true) => "remove",
                    _ => "noop",
                };
                resource(&item.key, action)
            })
            .collect())
    }

    /// Installs and removes what the plan of `items` says now. Returns 0
    /// when every command succeeded, and the log of each.
    pub fn apply(&self, items: &[Item]) -> (i32, String) {
        let resources = match self.plan(items) {
            Ok(resources) => resources,
            Err(error) => return (1, format!("{error}\n")),
        };
        let mut log = String::new();
        let mut exit_code = 0;
        for manager in [Manager::Brew, Manager::Cask, Manager::Pacman] {
            for remove in [false, true] {
                let action = if remove { "remove" } else { "create" };
                let names: Vec<&str> = items
                    .iter()
                    .zip(&resources)
                    .filter(|(item, resource)| {
                        item.manager == manager && resource["action"] == action
                    })
                    .map(|(item, _)| item.name.as_str())
                    .collect();
                if names.is_empty() {
                    continue;
                }
                let program = match self.program(manager) {
                    Ok(program) => program,
                    Err(error) => return (1, format!("{log}{error}\n")),
                };
                let args: &[&str] = match (manager, remove) {
                    (Manager::Brew, false) => &["install"],
                    (Manager::Brew, true) => &["uninstall"],
                    (Manager::Cask, false) => &["install", "--cask"],
                    (Manager::Cask, true) => &["uninstall", "--cask"],
                    (Manager::Pacman, false) => &["-S", "--noconfirm", "--needed"],
                    (Manager::Pacman, true) => &["-Rns", "--noconfirm"],
                };
                log.push_str(&format!(
                    "$ {} {} {}\n",
                    program.display(),
                    args.join(" "),
                    names.join(" ")
                ));
                match Command::new(&program)
                    .args(args)
                    .args(&names)
                    .stdin(Stdio::null())
                    .output()
                {
                    Ok(output) => {
                        log.push_str(&String::from_utf8_lossy(&output.stdout));
                        log.push_str(&String::from_utf8_lossy(&output.stderr));
                        if !output.status.success() {
                            exit_code = 1;
                        }
                    }
                    Err(error) => {
                        log.push_str(&format!("cannot run {}: {error}\n", program.display()));
                        exit_code = 1;
                    }
                }
            }
        }
        (exit_code, log)
    }
}

/// A resource of a mise plan.
pub fn resource(name: &str, action: &str) -> Value {
    json!({ "id": { "kind": "package", "name": name }, "action": action })
}

/// The installed packages of a manager, by name.
fn list(program: &Path, manager: Manager) -> Result<BTreeSet<String>, String> {
    let args: &[&str] = match manager {
        Manager::Brew => &["list", "--formula", "-1"],
        Manager::Cask => &["list", "--cask", "-1"],
        Manager::Pacman => &["-Qq"],
    };
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("cannot run {}: {error}", program.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} {} failed: {}",
            program.display(),
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The first candidate that exists. A bare name is looked up in `PATH`.
pub fn find(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find_map(|candidate| {
        if candidate.components().count() > 1 {
            return candidate.is_file().then(|| candidate.clone());
        }
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|dir| dir.join(candidate))
            .find(|path| path.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac() -> Native {
        Native {
            macos: true,
            ..Native::this()
        }
    }

    #[test]
    fn a_mac_takes_brew_items_at_latest_or_absent() {
        let body = b"# Rendered.\n\n[bootstrap.packages]\n\"brew:mise\" = \"latest\"\n\
            \"brew-cask:cursor\" = { state = \"absent\" }\n\"brew:jq\" = \"1.7\"\n\
            \"pacman:htop\" = \"latest\"\n\n[tools]\n\"node\" = \"24\"\n";
        let split = mac().split(body);
        assert_eq!(
            split.native,
            vec![
                Item {
                    key: "brew-cask:cursor".to_owned(),
                    manager: Manager::Cask,
                    name: "cursor".to_owned(),
                    absent: true,
                },
                Item {
                    key: "brew:mise".to_owned(),
                    manager: Manager::Brew,
                    name: "mise".to_owned(),
                    absent: false,
                },
            ]
        );
        let rest = split.mise.unwrap();
        assert!(rest.contains("\"brew:jq\" = \"1.7\""), "{rest}");
        assert!(!rest.contains("brew:mise"), "{rest}");
        assert!(rest.contains("node = \"24\""), "{rest}");
        assert_eq!(
            split.rest,
            vec![
                ("package".to_owned(), "brew:jq".to_owned()),
                ("package".to_owned(), "pacman:htop".to_owned()),
                ("tool".to_owned(), "node".to_owned()),
            ]
        );
    }

    #[test]
    fn linux_takes_pacman_items_and_leaves_aur_to_mise() {
        let native = Native {
            macos: false,
            ..Native::this()
        };
        let body =
            b"[bootstrap.packages]\n\"pacman:mise\" = \"latest\"\n\"aur:yay\" = \"latest\"\n";
        let split = native.split(body);
        assert_eq!(split.native.len(), 1);
        assert_eq!(split.native[0].manager, Manager::Pacman);
        assert_eq!(
            split.rest,
            vec![("package".to_owned(), "aur:yay".to_owned())]
        );
    }

    #[test]
    fn a_config_without_native_items_goes_to_mise_unchanged() {
        let split = mac().split(b"[tools]\nnode = \"24\"\n");
        assert!(split.native.is_empty());
        assert_eq!(split.mise, None);
        let split = mac().split(b"not toml [");
        assert_eq!(split, Split::default());
    }

    #[test]
    fn no_homebrew_fails_the_native_items() {
        let native = Native {
            macos: true,
            brew: vec!["/nonexistent/brew".into()],
            pacman: vec![],
        };
        let split = native.split(b"[bootstrap.packages]\n\"brew:mise\" = \"latest\"\n");
        assert_eq!(native.plan(&split.native), Err(NO_HOMEBREW.to_owned()));
        assert_eq!(native.apply(&split.native), (1, format!("{NO_HOMEBREW}\n")));
    }
}
