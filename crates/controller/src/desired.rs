//! The desired state and the mise bootstrap config each host gets from it.
//!
//! A host's items are the items of its roles, with its adjustments on top.
//! The approved state lives in the `role_items`, `host_roles`, and
//! `host_adjustments` tables. A draft change's proposed state is the
//! approved state with the change's edits applied in order.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;
use uuid::Uuid;

/// What an item installs: a system package through mise bootstrap, or a
/// mise tool.
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

    fn parse(text: &str) -> Option<Self> {
        match text {
            "package" => Some(Self::Package),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }
}

/// An item's key: its kind and name.
pub type Key = (Kind, String);

/// The package version that means "must not be installed".
pub const ABSENT: &str = "absent";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub kind: Kind,
    pub name: String,
    pub version: String,
}

/// What a host changes on top of its roles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Adjustment {
    /// Adds the item, or sets its version, for this host.
    Set { version: String },
    /// Drops an item that a role declares.
    Remove,
}

/// One edit of a change. Each is stored as JSON in `change_edits` and in
/// its `change.edited` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    /// Adds an item to a role or changes its version.
    RoleItemSet {
        role: Uuid,
        kind: Kind,
        name: String,
        version: String,
    },
    RoleItemRemove {
        role: Uuid,
        kind: Kind,
        name: String,
    },
    HostRoleAdd {
        host: Uuid,
        role: Uuid,
    },
    HostRoleRemove {
        host: Uuid,
        role: Uuid,
    },
    /// Adds an item to one host, or changes its version there.
    HostItemSet {
        host: Uuid,
        kind: Kind,
        name: String,
        version: String,
    },
    /// Drops an item that the host's roles declare, on this host only.
    HostItemRemove {
        host: Uuid,
        kind: Kind,
        name: String,
    },
    /// Drops the host's adjustment of an item, so its roles decide again.
    HostItemReset {
        host: Uuid,
        kind: Kind,
        name: String,
    },
}

impl Edit {
    /// The role and host the edit names.
    pub fn targets(&self) -> (Option<Uuid>, Option<Uuid>) {
        match self {
            Self::RoleItemSet { role, .. } | Self::RoleItemRemove { role, .. } => {
                (Some(*role), None)
            }
            Self::HostRoleAdd { host, role } | Self::HostRoleRemove { host, role } => {
                (Some(*role), Some(*host))
            }
            Self::HostItemSet { host, .. }
            | Self::HostItemRemove { host, .. }
            | Self::HostItemReset { host, .. } => (None, Some(*host)),
        }
    }

    /// Whether names and versions are well formed: not empty, at most 200
    /// characters, no control characters, and `absent` only for packages.
    pub fn is_valid(&self) -> bool {
        let text = |value: &str| {
            !value.trim().is_empty()
                && value.len() <= 200
                && value == value.trim()
                && !value.chars().any(char::is_control)
        };
        match self {
            Self::RoleItemSet {
                kind,
                name,
                version,
                ..
            }
            | Self::HostItemSet {
                kind,
                name,
                version,
                ..
            } => text(name) && text(version) && (*kind == Kind::Package || version != ABSENT),
            Self::RoleItemRemove { name, .. }
            | Self::HostItemRemove { name, .. }
            | Self::HostItemReset { name, .. } => text(name),
            Self::HostRoleAdd { .. } | Self::HostRoleRemove { .. } => true,
        }
    }
}

/// The whole desired state, approved or proposed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub role_names: BTreeMap<Uuid, String>,
    pub role_items: BTreeMap<Uuid, BTreeMap<Key, String>>,
    pub host_roles: BTreeMap<Uuid, BTreeSet<Uuid>>,
    pub adjustments: BTreeMap<Uuid, BTreeMap<Key, Adjustment>>,
}

impl State {
    /// Reads the approved state.
    pub async fn load(connection: &mut PgConnection) -> sqlx::Result<Self> {
        let mut state = Self::default();
        let roles: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, name FROM roles")
            .fetch_all(&mut *connection)
            .await?;
        state.role_names.extend(roles);
        let items: Vec<(Uuid, String, String, String)> =
            sqlx::query_as("SELECT role_id, kind, name, version FROM role_items")
                .fetch_all(&mut *connection)
                .await?;
        for (role, kind, name, version) in items {
            if let Some(kind) = Kind::parse(&kind) {
                state
                    .role_items
                    .entry(role)
                    .or_default()
                    .insert((kind, name), version);
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
        Ok(state)
    }

    /// Applies one edit.
    pub fn apply(&mut self, edit: &Edit) {
        match edit.clone() {
            Edit::RoleItemSet {
                role,
                kind,
                name,
                version,
            } => {
                self.role_items
                    .entry(role)
                    .or_default()
                    .insert((kind, name), version);
            }
            Edit::RoleItemRemove { role, kind, name } => {
                if let Some(items) = self.role_items.get_mut(&role) {
                    items.remove(&(kind, name));
                }
            }
            Edit::HostRoleAdd { host, role } => {
                self.host_roles.entry(host).or_default().insert(role);
            }
            Edit::HostRoleRemove { host, role } => {
                if let Some(roles) = self.host_roles.get_mut(&host) {
                    roles.remove(&role);
                }
            }
            Edit::HostItemSet {
                host,
                kind,
                name,
                version,
            } => {
                self.adjustments
                    .entry(host)
                    .or_default()
                    .insert((kind, name), Adjustment::Set { version });
            }
            Edit::HostItemRemove { host, kind, name } => {
                self.adjustments
                    .entry(host)
                    .or_default()
                    .insert((kind, name), Adjustment::Remove);
            }
            Edit::HostItemReset { host, kind, name } => {
                if let Some(adjustments) = self.adjustments.get_mut(&host) {
                    adjustments.remove(&(kind, name));
                }
            }
        }
    }

    /// The roles of a host, by name.
    pub fn roles_of(&self, host: Uuid) -> Vec<(Uuid, String)> {
        let mut roles: Vec<(Uuid, String)> = self
            .host_roles
            .get(&host)
            .into_iter()
            .flatten()
            .filter_map(|role| Some((*role, self.role_names.get(role)?.clone())))
            .collect();
        roles.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        roles
    }

    /// The items of a host: the items of its roles, then its adjustments.
    /// When two roles declare the same item with different versions, the
    /// role whose name sorts last wins.
    pub fn host_items(&self, host: Uuid) -> BTreeMap<Key, String> {
        let mut items = BTreeMap::new();
        for (role, _) in self.roles_of(host) {
            if let Some(role_items) = self.role_items.get(&role) {
                items.extend(role_items.iter().map(|(k, v)| (k.clone(), v.clone())));
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

/// A TOML basic string.
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

/// The mise bootstrap config of a set of items: packages under
/// `[bootstrap.packages]`, tools under `[tools]`.
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

/// The hex SHA-256 of a config body, as the agent computes it.
pub fn hash(body: &str) -> String {
    hex(&Sha256::digest(body.as_bytes()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The name of the config of the approved state.
pub const CURRENT: &str = "current";

/// The name of a change's config.
pub fn change_config(change: Uuid) -> String {
    format!("change/{change}")
}

/// The packages of a set of items that count as declared for a package
/// manager: present packages, without a manager prefix, whose prefix is
/// none or this manager.
pub fn declared_packages(items: &BTreeMap<Key, String>, manager: &str) -> BTreeSet<String> {
    items
        .iter()
        .filter(|((kind, _), version)| *kind == Kind::Package && version.as_str() != ABSENT)
        .filter_map(|((_, name), _)| match name.split_once(':') {
            Some((prefix, package)) if prefix == manager => Some(package.to_owned()),
            Some(_) => None,
            None => Some(name.clone()),
        })
        .collect()
}

/// The draft change and its edits in order, if there is a draft.
pub async fn draft(connection: &mut PgConnection) -> sqlx::Result<Option<(Uuid, Vec<Edit>)>> {
    let Some(id): Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM changes WHERE status = 'draft'")
            .fetch_optional(&mut *connection)
            .await?
    else {
        return Ok(None);
    };
    let edits: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT edit FROM change_edits WHERE change_id = $1 ORDER BY id")
            .bind(id)
            .fetch_all(&mut *connection)
            .await?;
    Ok(Some((
        id,
        edits
            .into_iter()
            .filter_map(|edit| serde_json::from_value(edit).ok())
            .collect(),
    )))
}

/// The approved state, and the draft's id and proposed state if there is a
/// draft.
pub async fn states(connection: &mut PgConnection) -> sqlx::Result<(State, Option<(Uuid, State)>)> {
    let approved = State::load(&mut *connection).await?;
    let proposed = draft(&mut *connection).await?.map(|(id, edits)| {
        let mut proposed = approved.clone();
        for edit in &edits {
            proposed.apply(edit);
        }
        (id, proposed)
    });
    Ok((approved, proposed))
}

/// The configs the controller sends a host, by name: `current` always, and
/// `change/<id>` when the draft changes what the host gets.
pub fn host_configs(
    host: Uuid,
    approved: &State,
    proposed: Option<&(Uuid, State)>,
) -> BTreeMap<String, String> {
    let current = render(&approved.host_items(host));
    let mut configs = BTreeMap::new();
    if let Some((change, proposed)) = proposed {
        let body = render(&proposed.host_items(host));
        if body != current {
            configs.insert(change_config(*change), body);
        }
    }
    configs.insert(CURRENT.to_owned(), current);
    configs
}

/// The hash of a whole set of configs, for OpAMP's `config_hash`.
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

    #[test]
    fn host_items_merge_roles_then_adjustments() {
        let (base, dev, host) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let mut state = State::default();
        state.role_names.insert(base, "base".to_owned());
        state.role_names.insert(dev, "dev".to_owned());
        for edit in [
            Edit::RoleItemSet {
                role: base,
                kind: Kind::Package,
                name: "git".to_owned(),
                version: "latest".to_owned(),
            },
            Edit::RoleItemSet {
                role: dev,
                kind: Kind::Package,
                name: "git".to_owned(),
                version: "2".to_owned(),
            },
            Edit::RoleItemSet {
                role: dev,
                kind: Kind::Tool,
                name: "node".to_owned(),
                version: "24".to_owned(),
            },
            Edit::RoleItemSet {
                role: dev,
                kind: Kind::Package,
                name: "sl".to_owned(),
                version: "latest".to_owned(),
            },
            Edit::HostRoleAdd { host, role: base },
            Edit::HostRoleAdd { host, role: dev },
            Edit::HostItemRemove {
                host,
                kind: Kind::Package,
                name: "sl".to_owned(),
            },
            Edit::HostItemSet {
                host,
                kind: Kind::Package,
                name: "pacman:htop".to_owned(),
                version: ABSENT.to_owned(),
            },
        ] {
            state.apply(&edit);
        }
        let items = state.host_items(host);
        assert_eq!(
            items,
            BTreeMap::from([
                (item(Kind::Package, "git"), "2".to_owned()),
                (item(Kind::Package, "pacman:htop"), ABSENT.to_owned()),
                (item(Kind::Tool, "node"), "24".to_owned()),
            ])
        );
        assert_eq!(
            render(&items),
            "# Rendered by the sewa controller. Do not edit.\n\
             \n[bootstrap.packages]\n\
             \"git\" = \"2\"\n\
             \"pacman:htop\" = { state = \"absent\" }\n\
             \n[tools]\n\
             \"node\" = \"24\"\n"
        );
        assert_eq!(
            declared_packages(&items, "pacman"),
            BTreeSet::from(["git".to_owned()])
        );

        state.apply(&Edit::HostItemReset {
            host,
            kind: Kind::Package,
            name: "sl".to_owned(),
        });
        assert!(
            state
                .host_items(host)
                .contains_key(&item(Kind::Package, "sl"))
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
    fn edits_are_checked() {
        let host = Uuid::new_v4();
        let set = |kind, name: &str, version: &str| Edit::HostItemSet {
            host,
            kind,
            name: name.to_owned(),
            version: version.to_owned(),
        };
        assert!(set(Kind::Package, "jq", "latest").is_valid());
        assert!(set(Kind::Package, "jq", ABSENT).is_valid());
        assert!(!set(Kind::Tool, "node", ABSENT).is_valid());
        assert!(!set(Kind::Package, " jq", "latest").is_valid());
        assert!(!set(Kind::Package, "jq\n", "latest").is_valid());
        assert!(!set(Kind::Package, "jq", "").is_valid());
    }

    #[test]
    fn edits_read_as_json() {
        let edit: Edit = serde_json::from_value(serde_json::json!({
            "op": "role_item_set",
            "role": Uuid::nil(),
            "kind": "package",
            "name": "jq",
            "version": "latest",
        }))
        .unwrap();
        assert_eq!(
            edit,
            Edit::RoleItemSet {
                role: Uuid::nil(),
                kind: Kind::Package,
                name: "jq".to_owned(),
                version: "latest".to_owned(),
            }
        );
    }
}
