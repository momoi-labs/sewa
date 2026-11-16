use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

pub const BOOTSTRAP_ROLE: &str = "bootstrap";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Package,
    Tool,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::Tool => "tool",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "package" => Some(Self::Package),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }
}

pub type Key = (Kind, String);

pub const ABSENT: &str = "absent";

pub const OSES: [&str; 2] = ["linux", "macos"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub version: String,
    pub os: Option<String>,
}

impl Spec {
    pub fn new(version: &str) -> Self {
        Self {
            version: version.to_owned(),
            os: None,
        }
    }

    fn fits(&self, os: Option<&str>) -> bool {
        self.os.is_none() || self.os.as_deref() == os
    }
}

fn version_from_json<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let version = String::deserialize(deserializer)?;
    Ok(if version.trim().eq_ignore_ascii_case("latest") {
        "latest".to_owned()
    } else {
        version
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub kind: Kind,
    pub name: String,
    #[serde(deserialize_with = "version_from_json")]
    pub version: String,
    #[serde(default)]
    pub os: Option<String>,
}

impl Item {
    pub fn is_valid(&self) -> bool {
        valid_text(&self.name)
            && valid_text(&self.version)
            && (self.kind == Kind::Package || self.version != ABSENT)
            && self.os.as_deref().is_none_or(|os| OSES.contains(&os))
    }
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 200
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Adjustment {
    Set { version: String },
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    HostRoleAdd {
        role: Uuid,
    },
    HostRoleRemove {
        role: Uuid,
    },
    HostItemSet {
        kind: Kind,
        name: String,
        #[serde(deserialize_with = "version_from_json")]
        version: String,
    },
    HostItemRemove {
        kind: Kind,
        name: String,
    },
    HostItemReset {
        kind: Kind,
        name: String,
    },
}

impl Edit {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::HostItemSet {
                kind,
                name,
                version,
            } => Item {
                kind: *kind,
                name: name.clone(),
                version: version.clone(),
                os: None,
            }
            .is_valid(),
            Self::HostItemRemove { name, .. } | Self::HostItemReset { name, .. } => {
                valid_text(name)
            }
            Self::HostRoleAdd { .. } | Self::HostRoleRemove { .. } => true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub role_names: BTreeMap<Uuid, String>,
    pub builtin: Option<Uuid>,
    pub role_items: BTreeMap<Uuid, BTreeMap<Key, Spec>>,
    pub host_roles: BTreeMap<Uuid, BTreeSet<Uuid>>,
    pub adjustments: BTreeMap<Uuid, BTreeMap<Key, Adjustment>>,
    pub host_os: BTreeMap<Uuid, String>,
}

impl State {
    pub async fn load(connection: &mut PgConnection) -> sqlx::Result<Self> {
        let mut state = Self::default();
        let roles: Vec<(Uuid, String, bool)> =
            sqlx::query_as("SELECT id, name, builtin FROM roles")
                .fetch_all(&mut *connection)
                .await?;
        for (id, name, builtin) in roles {
            if builtin {
                state.builtin = Some(id);
            }
            state.role_names.insert(id, name);
        }
        let items: Vec<(Uuid, String, String, String, Option<String>)> =
            sqlx::query_as("SELECT role_id, kind, name, version, os FROM role_items")
                .fetch_all(&mut *connection)
                .await?;
        for (role, kind, name, version, os) in items {
            if let Some(kind) = Kind::parse(&kind) {
                state
                    .role_items
                    .entry(role)
                    .or_default()
                    .insert((kind, name), Spec { version, os });
            }
        }
        let host_roles: Vec<(Uuid, Uuid)> =
            sqlx::query_as("SELECT host_id, role_id FROM host_roles")
                .fetch_all(&mut *connection)
                .await?;
        for (host, role) in host_roles {
            state.host_roles.entry(host).or_default().insert(role);
        }
        let adjustments: Vec<(Uuid, String, String, String, Option<String>)> =
            sqlx::query_as("SELECT host_id, kind, name, action, version FROM host_adjustments")
                .fetch_all(&mut *connection)
                .await?;
        for (host, kind, name, action, version) in adjustments {
            let adjustment = match (action.as_str(), version) {
                ("set", Some(version)) => Adjustment::Set { version },
                ("remove", _) => Adjustment::Remove,
                _ => continue,
            };
            if let Some(kind) = Kind::parse(&kind) {
                state
                    .adjustments
                    .entry(host)
                    .or_default()
                    .insert((kind, name), adjustment);
            }
        }
        let hosts: Vec<(Uuid, String)> =
            sqlx::query_as("SELECT id, os FROM hosts WHERE os IS NOT NULL")
                .fetch_all(&mut *connection)
                .await?;
        state.host_os.extend(hosts);
        Ok(state)
    }

    pub fn roles_of(&self, host: Uuid) -> Vec<(Uuid, String)> {
        let mut roles: Vec<(Uuid, String)> = self
            .host_roles
            .get(&host)
            .into_iter()
            .flatten()
            .filter(|role| Some(**role) != self.builtin)
            .filter_map(|role| Some((*role, self.role_names.get(role)?.clone())))
            .collect();
        roles.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        roles
    }

    pub fn host_items(&self, host: Uuid) -> BTreeMap<Key, String> {
        let os = self.host_os.get(&host).map(String::as_str);
        let mut items = BTreeMap::new();
        let builtin = self.builtin.into_iter().flat_map(|role| {
            self.role_items
                .get(&role)
                .into_iter()
                .flatten()
                .map(move |((kind, name), spec)| ((*kind, bootstrap_key(*kind, name, os)), spec))
        });
        let roles = self.roles_of(host).into_iter().flat_map(|(role, _)| {
            self.role_items
                .get(&role)
                .into_iter()
                .flatten()
                .map(|(key, spec)| (key.clone(), spec))
        });
        for (key, spec) in builtin.chain(roles) {
            if spec.fits(os) {
                items.insert(key, spec.version.clone());
            }
        }
        for (key, adjustment) in self.adjustments.get(&host).into_iter().flatten() {
            match adjustment {
                Adjustment::Set { version } => {
                    items.insert(key.clone(), version.clone());
                }
                Adjustment::Remove => {
                    items.remove(key);
                }
            }
        }
        items
    }
}

pub fn bootstrap_key(kind: Kind, name: &str, os: Option<&str>) -> String {
    match (kind, name.contains(':'), os) {
        (Kind::Package, false, Some("macos")) => format!("brew:{name}"),
        (Kind::Package, false, Some("linux")) => format!("pacman:{name}"),
        _ => name.to_owned(),
    }
}

pub async fn ensure_bootstrap(pool: &sqlx::PgPool) -> sqlx::Result<()> {
    let mut transaction = pool.begin().await?;
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM roles WHERE builtin FOR UPDATE")
        .fetch_optional(&mut *transaction)
        .await?;
    match id {
        Some(id) => {
            sqlx::query("UPDATE roles SET name = $2 WHERE id = $1 AND name <> $2")
                .bind(id)
                .bind(BOOTSTRAP_ROLE)
                .execute(&mut *transaction)
                .await?;
        }
        None => {
            let id: Uuid = sqlx::query_scalar(
                "INSERT INTO roles (name, builtin) VALUES ($1, true)
                 ON CONFLICT (name) DO UPDATE SET builtin = true
                 RETURNING id",
            )
            .bind(BOOTSTRAP_ROLE)
            .fetch_one(&mut *transaction)
            .await?;
            sqlx::query(
                "INSERT INTO role_items (role_id, kind, name, version)
                 VALUES ($1, 'package', 'mise', 'latest') ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        }
    }
    transaction.commit().await
}

fn quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            character if character.is_control() => {
                quoted.push_str(&format!("\\u{:04X}", character as u32));
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

pub fn render(items: &BTreeMap<Key, String>) -> String {
    let mut body = String::from("# Rendered by the sewa controller. Do not edit.\n");
    let section = |kind: Kind| {
        items
            .iter()
            .filter(move |((item_kind, _), _)| *item_kind == kind)
            .map(|((_, name), version)| (name, version))
    };
    let mut packages = section(Kind::Package).peekable();
    if packages.peek().is_some() {
        body.push_str("\n[bootstrap.packages]\n");
        for (name, version) in packages {
            if version == ABSENT {
                body.push_str(&format!("{} = {{ state = \"absent\" }}\n", quote(name)));
            } else {
                body.push_str(&format!("{} = {}\n", quote(name), quote(version)));
            }
        }
    }
    let mut tools = section(Kind::Tool).peekable();
    if tools.peek().is_some() {
        body.push_str("\n[tools]\n");
        for (name, version) in tools {
            body.push_str(&format!("{} = {}\n", quote(name), quote(version)));
        }
    }
    body
}

pub fn hash(body: &str) -> String {
    hex(&Sha256::digest(body.as_bytes()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub const CURRENT: &str = "current";

pub fn declared_packages(items: &BTreeMap<Key, String>, manager: &str) -> BTreeSet<String> {
    items
        .iter()
        .filter(|((kind, _), version)| *kind == Kind::Package && version.as_str() != ABSENT)
        .filter_map(|((_, name), _)| match name.split_once(':') {
            Some((prefix, package))
                if prefix == manager || (manager == "pacman" && prefix == "aur") =>
            {
                Some(package.to_owned())
            }
            Some(_) => None,
            None => Some(name.clone()),
        })
        .collect()
}

pub async fn save(connection: &mut PgConnection, host: Uuid, edit: &Edit) -> sqlx::Result<()> {
    let query = match edit {
        Edit::HostRoleAdd { role } => sqlx::query(
            "INSERT INTO host_roles (host_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(host)
        .bind(role),
        Edit::HostRoleRemove { role } => {
            sqlx::query("DELETE FROM host_roles WHERE host_id = $1 AND role_id = $2")
                .bind(host)
                .bind(role)
        }
        Edit::HostItemSet {
            kind,
            name,
            version,
        } => sqlx::query(
            "INSERT INTO host_adjustments (host_id, kind, name, action, version)
             VALUES ($1, $2, $3, 'set', $4)
             ON CONFLICT (host_id, kind, name) DO UPDATE SET action = 'set', version = $4",
        )
        .bind(host)
        .bind(kind.as_str())
        .bind(name)
        .bind(version),
        Edit::HostItemRemove { kind, name } => sqlx::query(
            "INSERT INTO host_adjustments (host_id, kind, name, action, version)
             VALUES ($1, $2, $3, 'remove', NULL)
             ON CONFLICT (host_id, kind, name) DO UPDATE SET action = 'remove', version = NULL",
        )
        .bind(host)
        .bind(kind.as_str())
        .bind(name),
        Edit::HostItemReset { kind, name } => sqlx::query(
            "DELETE FROM host_adjustments WHERE host_id = $1 AND kind = $2 AND name = $3",
        )
        .bind(host)
        .bind(kind.as_str())
        .bind(name),
    };
    query.execute(connection).await?;
    Ok(())
}

pub fn host_config(host: Uuid, state: &State) -> String {
    render(&state.host_items(host))
}

pub fn configs_hash(configs: &BTreeMap<String, String>) -> Vec<u8> {
    let mut digest = Sha256::new();
    for (name, body) in configs {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(hash(body).as_bytes());
        digest.update(b"\n");
    }
    digest.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: Kind, name: &str) -> Key {
        (kind, name.to_owned())
    }

    fn spec(version: &str, os: Option<&str>) -> Spec {
        Spec {
            version: version.to_owned(),
            os: os.map(str::to_owned),
        }
    }

    fn state(os: &str) -> (State, Uuid) {
        let (bootstrap, base, dev, host) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let mut state = State {
            builtin: Some(bootstrap),
            ..State::default()
        };
        state
            .role_names
            .insert(bootstrap, BOOTSTRAP_ROLE.to_owned());
        state.role_names.insert(base, "base".to_owned());
        state.role_names.insert(dev, "dev".to_owned());
        state.role_items.insert(
            bootstrap,
            BTreeMap::from([(item(Kind::Package, "mise"), Spec::new("latest"))]),
        );
        state.role_items.insert(
            base,
            BTreeMap::from([(item(Kind::Package, "git"), Spec::new("latest"))]),
        );
        state.role_items.insert(
            dev,
            BTreeMap::from([
                (item(Kind::Package, "git"), Spec::new("2")),
                (item(Kind::Tool, "node"), Spec::new("24")),
                (item(Kind::Package, "sl"), Spec::new("latest")),
                (
                    item(Kind::Package, "brew-cask:cursor"),
                    spec("latest", Some("macos")),
                ),
                (
                    item(Kind::Package, "aur:cursor-bin"),
                    spec("latest", Some("linux")),
                ),
            ]),
        );
        state.host_roles.insert(host, BTreeSet::from([base, dev]));
        state.host_os.insert(host, os.to_owned());
        (state, host)
    }

    #[test]
    fn host_items_merge_bootstrap_roles_then_adjustments() {
        let (mut state, host) = state("linux");
        state.adjustments.insert(
            host,
            BTreeMap::from([
                (item(Kind::Package, "sl"), Adjustment::Remove),
                (
                    item(Kind::Package, "pacman:htop"),
                    Adjustment::Set {
                        version: ABSENT.to_owned(),
                    },
                ),
            ]),
        );
        let items = state.host_items(host);
        assert_eq!(
            items,
            BTreeMap::from([
                (item(Kind::Package, "aur:cursor-bin"), "latest".to_owned()),
                (item(Kind::Package, "git"), "2".to_owned()),
                (item(Kind::Package, "pacman:htop"), ABSENT.to_owned()),
                (item(Kind::Package, "pacman:mise"), "latest".to_owned()),
                (item(Kind::Tool, "node"), "24".to_owned()),
            ])
        );
        assert_eq!(
            render(&items),
            "# Rendered by the sewa controller. Do not edit.\n\
             \n[bootstrap.packages]\n\
             \"aur:cursor-bin\" = \"latest\"\n\
             \"git\" = \"2\"\n\
             \"pacman:htop\" = { state = \"absent\" }\n\
             \"pacman:mise\" = \"latest\"\n\
             \n[tools]\n\
             \"node\" = \"24\"\n"
        );
        assert_eq!(
            state
                .roles_of(host)
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>(),
            ["base", "dev"]
        );
    }

    #[test]
    fn a_mac_gets_brew_for_bootstrap_and_only_its_os_items() {
        let (state, host) = state("macos");
        let items = state.host_items(host);
        assert!(items.contains_key(&item(Kind::Package, "brew:mise")));
        assert!(items.contains_key(&item(Kind::Package, "brew-cask:cursor")));
        assert!(!items.contains_key(&item(Kind::Package, "aur:cursor-bin")));
        assert!(!items.contains_key(&item(Kind::Package, "mise")));
    }

    #[test]
    fn bootstrap_keys_keep_a_prefix_and_tools() {
        assert_eq!(
            bootstrap_key(Kind::Package, "mise", Some("macos")),
            "brew:mise"
        );
        assert_eq!(
            bootstrap_key(Kind::Package, "mise", Some("linux")),
            "pacman:mise"
        );
        assert_eq!(
            bootstrap_key(Kind::Package, "brew-cask:x", Some("macos")),
            "brew-cask:x"
        );
        assert_eq!(bootstrap_key(Kind::Tool, "node", Some("macos")), "node");
    }

    #[test]
    fn aur_packages_count_as_declared_for_pacman() {
        let items = BTreeMap::from([
            (item(Kind::Package, "aur:cursor-bin"), "latest".to_owned()),
            (item(Kind::Package, "pacman:git"), "latest".to_owned()),
            (item(Kind::Package, "jq"), "latest".to_owned()),
            (item(Kind::Package, "brew:wget"), "latest".to_owned()),
            (item(Kind::Package, "pacman:sl"), ABSENT.to_owned()),
        ]);
        assert_eq!(
            declared_packages(&items, "pacman"),
            BTreeSet::from(["cursor-bin".to_owned(), "git".to_owned(), "jq".to_owned()])
        );
        assert_eq!(
            declared_packages(&items, "brew"),
            BTreeSet::from(["jq".to_owned(), "wget".to_owned()])
        );
    }

    #[test]
    fn an_empty_state_renders_only_the_header() {
        assert_eq!(
            render(&BTreeMap::new()),
            "# Rendered by the sewa controller. Do not edit.\n"
        );
    }

    #[test]
    fn quotes_keys_and_values() {
        assert_eq!(quote(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(quote("a\u{1}"), "\"a\\u0001\"");
    }

    #[test]
    fn items_and_edits_are_checked() {
        let valid = |kind, name: &str, version: &str, os: Option<&str>| {
            Item {
                kind,
                name: name.to_owned(),
                version: version.to_owned(),
                os: os.map(str::to_owned),
            }
            .is_valid()
        };
        assert!(valid(Kind::Package, "jq", "latest", None));
        assert!(valid(Kind::Package, "jq", ABSENT, Some("macos")));
        assert!(!valid(Kind::Package, "jq", "latest", Some("windows")));
        assert!(!valid(Kind::Tool, "node", ABSENT, None));
        assert!(!valid(Kind::Package, " jq", "latest", None));
        assert!(!valid(Kind::Package, "jq\n", "latest", None));
        assert!(!valid(Kind::Package, "jq", "", None));
        assert!(
            !Edit::HostItemReset {
                kind: Kind::Package,
                name: String::new()
            }
            .is_valid()
        );
    }

    #[test]
    fn edits_read_as_json() {
        let edit: Edit = serde_json::from_value(serde_json::json!({
            "op": "host_item_set",
            "kind": "package",
            "name": "jq",
            "version": "latest",
        }))
        .unwrap();
        assert_eq!(
            edit,
            Edit::HostItemSet {
                kind: Kind::Package,
                name: "jq".to_owned(),
                version: "latest".to_owned(),
            }
        );
    }
}
