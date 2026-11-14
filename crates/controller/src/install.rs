//! The install script at `GET /install`, the agent binaries it downloads
//! from `GET /files/{name}`, and the one-line command the API returns.
//!
//! Adapted from pg-yard's `crates/cell/src/install.rs` and
//! `crates/cell/src/probe_files.rs`. The script holds the controller's
//! address, the checksums of the binaries, and the approver keys of the
//! moment, which it pins into the agent: an agent applies only what one of
//! them signed, and only the install pins keys. The install token is the
//! script's argument: it never goes into a URL, so no access log sees it.
//! The script downloads `sewa-agent` for the machine's OS and architecture,
//! checks it against the SHA-256 in the script, enrolls, and installs the
//! service:
//!
//! - Linux: a systemd unit that runs the agent as root.
//! - macOS: a launchd daemon that runs the agent as a user the script asks
//!   for. The default is the user who runs the installer.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use sewa_approval::Pinned;
use sha2::{Digest, Sha256};

/// The machines the install script installs on, as Rust targets.
pub const TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

/// The name of the agent binary for `target`.
pub fn agent_binary(target: &str) -> String {
    format!("sewa-agent-{target}")
}

/// The agent binaries the controller serves, by file name.
#[derive(Default)]
pub struct AgentFiles(HashMap<String, Bytes>);

impl AgentFiles {
    /// Reads the binary of every target in [`TARGETS`] from `dir`. A
    /// missing file stops the controller, so it never hands out a command
    /// that cannot finish.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let mut files = HashMap::new();
        for target in TARGETS {
            let name = agent_binary(target);
            let bytes = std::fs::read(dir.join(&name))
                .with_context(|| format!("cannot read {name} in {}", dir.display()))?;
            files.insert(name, Bytes::from(bytes));
        }
        Ok(Self(files))
    }

    /// The same files, with `bytes` as the binary of `target`. For tests.
    pub fn with_binary(mut self, target: &str, bytes: &[u8]) -> Self {
        self.0
            .insert(agent_binary(target), Bytes::copy_from_slice(bytes));
        self
    }

    /// The SHA-256 of each target's binary, in hex.
    pub fn sums(&self) -> Vec<(&'static str, String)> {
        TARGETS
            .iter()
            .filter_map(|target| {
                let bytes = self.0.get(&agent_binary(target))?;
                let sum = Sha256::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                Some((*target, sum))
            })
            .collect()
    }
}

/// `GET /files/{name}`: one agent binary, or 404. They hold no secret.
pub async fn serve_file(
    State(files): State<Arc<AgentFiles>>,
    UrlPath(name): UrlPath<String>,
) -> Response {
    match files.0.get(&name) {
        Some(bytes) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            bytes.clone(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// `GET /install`: the script, as text, so a browser shows it too. It is
/// rendered for each request, with the approver keys of that moment.
pub async fn serve_script(
    State(state): State<crate::AppState>,
) -> crate::error::ApiResult<Response> {
    let mut connection = state.pool.acquire().await?;
    let pinned = crate::passkeys::pinned(&mut connection, &state.public_url).await?;
    let script = script(&state.public_url, &state.agent_files.sums(), &pinned);
    Ok((
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        script,
    )
        .into_response())
}

/// Quotes a value for a POSIX shell: inside single quotes nothing expands,
/// and a single quote itself becomes `'\''`.
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// The command the API returns for a new host. The script runs as root on
/// both systems: it installs a system service.
pub fn command(public_url: &str, token: &str) -> String {
    let script_url = sh_quote(&format!("{public_url}/install"));
    let token = sh_quote(token);
    format!("curl -fsSL {script_url} | sudo sh -s -- {token}")
}

/// The install script for a controller at `public_url`, which serves
/// binaries with `sums`, and whose agents trust the `pinned` keys.
pub fn script(public_url: &str, sums: &[(&str, String)], pinned: &Pinned) -> String {
    let server = sh_quote(public_url);
    let approvers = sh_quote(&serde_json::to_string(pinned).expect("keys serialize"));
    let files_url = sh_quote(&format!("{public_url}/files"));
    let sums: String = sums
        .iter()
        .map(|(target, sum)| format!("    {target}) sum={} ;;\n", sh_quote(sum)))
        .collect();
    format!(
        r#"#!/bin/sh
# Installs sewa-agent, enrolls this machine with the sewa controller, and
# starts the agent as a service. Run it as root, with the install token as
# its only argument.
set -eu

server={server}
bin=/usr/local/bin/sewa-agent
# The approver keys: the agent applies only what one of them signed.
approvers={approvers}

fail() {{
  echo "sewa: $*" >&2
  exit 1
}}

sha256() {{
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}}

install_linux() {{
  "$bin" enroll --server "$server" --token "$token"
  printf '%s\n' "$approvers" | "$bin" pin
  cat >/etc/systemd/system/sewa-agent.service <<'UNIT'
[Unit]
Description=sewa agent
Wants=network-online.target
After=network-online.target

[Service]
Type=exec
ExecStart=/usr/local/bin/sewa-agent run
Restart=on-failure
RestartSec=10s
StateDirectory=sewa
StateDirectoryMode=0700

[Install]
WantedBy=multi-user.target
UNIT
  systemctl daemon-reload
  systemctl enable sewa-agent.service >/dev/null 2>&1
  systemctl restart sewa-agent.service
  echo "sewa: sewa-agent runs as root (systemd unit sewa-agent.service)"
}}

install_macos() {{
  default=${{SUDO_USER:-$(id -un)}}
  user=$default
  if [ -r /dev/tty ]; then
    printf 'sewa: which user runs the agent? [%s] ' "$default" >/dev/tty
    read -r answer </dev/tty || answer=
    [ -n "$answer" ] && user=$answer
  fi
  id -u "$user" >/dev/null 2>&1 || fail "no user $user"
  home=$(dscl . -read "/Users/$user" NFSHomeDirectory | sed 's/^NFSHomeDirectory: //')
  sudo -H -u "$user" "$bin" enroll --server "$server" --token "$token"
  printf '%s\n' "$approvers" | sudo -H -u "$user" "$bin" pin
  plist=/Library/LaunchDaemons/sewa.agent.plist
  cat >"$plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>sewa.agent</string>
  <key>ProgramArguments</key>
  <array><string>/usr/local/bin/sewa-agent</string><string>run</string></array>
  <key>UserName</key><string>$user</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardErrorPath</key><string>$home/Library/Logs/sewa-agent.log</string>
</dict>
</plist>
PLIST
  chmod 0644 "$plist"
  launchctl bootout system/sewa.agent >/dev/null 2>&1 || true
  launchctl bootstrap system "$plist"
  echo "sewa: sewa-agent runs as $user (launchd daemon sewa.agent)"
}}

# main runs on the last line, so a download that stops halfway runs nothing.
main() {{
  if [ $# -ne 1 ] || [ -z "$1" ]; then
    fail "give the install token as the only argument"
  fi
  token=$1
  [ "$(id -u)" -eq 0 ] || fail "run the install as root"
  case $(uname -s) in
    Linux) os=linux; t=unknown-linux-musl ;;
    Darwin) os=macos; t=apple-darwin ;;
    *) fail "sewa-agent does not run on $(uname -s)" ;;
  esac
  case $(uname -m) in
    x86_64 | amd64) t=x86_64-$t ;;
    aarch64 | arm64) t=aarch64-$t ;;
    *) fail "sewa-agent does not run on $(uname -m)" ;;
  esac
  case $t in
{sums}    *) fail "the controller has no sewa-agent for $t" ;;
  esac
  dir=$(mktemp -d)
  trap 'rm -rf "$dir"' EXIT
  curl -fsS -o "$dir/sewa-agent" {files_url}/"sewa-agent-$t"
  [ "$(sha256 "$dir/sewa-agent")" = "$sum" ] \
    || fail "the sewa-agent download does not match its checksum"
  mkdir -p /usr/local/bin
  install -m 0755 "$dir/sewa-agent" "$bin"
  "install_$os"
}}

main "$@"
"#
    )
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::process::{Command, Output, Stdio};

    use super::*;

    /// Runs `sh` with `args` and the script on stdin.
    fn sh(args: &[&str], script: &str) -> Output {
        let mut child = Command::new("sh")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    #[test]
    fn the_command_pipes_the_script_into_a_root_shell_with_the_token() {
        assert_eq!(
            command("http://sewa.example:8080", "tok'123"),
            r"curl -fsSL 'http://sewa.example:8080/install' | sudo sh -s -- 'tok'\''123'"
        );
    }

    #[test]
    fn the_script_carries_the_server_and_the_sums() {
        let script = script(
            "http://sewa.example:8080",
            &[("x86_64-unknown-linux-musl", "ab12".to_owned())],
            &Pinned::default(),
        );
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("\nserver='http://sewa.example:8080'\n"));
        assert!(script.contains("    x86_64-unknown-linux-musl) sum='ab12' ;;\n"));
        assert!(script.contains(
            "curl -fsS -o \"$dir/sewa-agent\" 'http://sewa.example:8080/files'/\"sewa-agent-$t\""
        ));
        assert!(script.ends_with("\nmain \"$@\"\n"));
    }

    #[test]
    fn the_script_parses_and_refuses_to_run_without_a_token() {
        let script = script("http://sewa.example:8080", &[], &Pinned::default());
        let parsed = sh(&["-n"], &script);
        assert!(
            parsed.status.success(),
            "{}",
            String::from_utf8_lossy(&parsed.stderr)
        );
        let run = sh(&["-s", "--"], &script);
        assert!(!run.status.success());
        assert!(String::from_utf8_lossy(&run.stderr).contains("install token"));
    }

    #[test]
    fn the_script_pins_the_approver_keys() {
        let pinned = Pinned {
            rp_id: "sewa.example".to_owned(),
            origin: "https://sewa.example".to_owned(),
            keys: vec![sewa_approval::Approver {
                id: "a'b".to_owned(),
                public_key: "key".to_owned(),
                login: "admin@example.com".to_owned(),
            }],
        };
        let script = script("https://sewa.example", &[], &pinned);
        assert!(script.contains(
            r#"approvers='{"rp_id":"sewa.example","origin":"https://sewa.example","keys":[{"id":"a'\''b","public_key":"key","login":"admin@example.com"}]}'"#
        ));
        assert!(script.contains("printf '%s\\n' \"$approvers\" | \"$bin\" pin\n"));
        // The shell reads the value back as the JSON.
        let output = Command::new("sh")
            .args([
                "-c",
                &format!(
                    "{}\nprintf %s \"$approvers\"",
                    script
                        .lines()
                        .find(|line| line.starts_with("approvers="))
                        .unwrap()
                ),
            ])
            .output()
            .unwrap();
        let read: Pinned = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(read, pinned);
    }

    #[test]
    fn sums_cover_the_loaded_binaries() {
        let files = AgentFiles::default().with_binary("aarch64-apple-darwin", b"agent");
        assert_eq!(
            files.sums(),
            vec![(
                "aarch64-apple-darwin",
                "d4f0bc5a29de06b510f9aa428f1eedba926012b591fef7a518e776a7c9bd1824".to_owned()
            )]
        );
    }
}
