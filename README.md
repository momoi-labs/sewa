# sewa (世話)

sewa (世話) is Japanese for "care, looking after". The tool looks after
machines over time and keeps each one as it was declared.

## Status

sewa is early. Hosts can enroll and report their status, and a web app
lists them. Roles and host adjustments are edited directly, and each
agent plans its desired state with mise. A person syncs the hosts they
pick and signs their plans with a passkey, and each agent applies its plan
only after it checks the signature against the keys its install pinned.
When the controller is down, each agent keeps checking its last desired
state and sends its reports once the controller is back. See
[docs/api.md](docs/api.md).

## What sewa will do

- Run a node agent, written in Rust, as a system service on each machine:
  systemd on Linux, launchd on macOS.
- Run a controller that holds the desired state and the approval flow. The
  controller stores the desired state, its history as events, and the
  approvals in its database. An encrypted backup of that database is kept
  off the host.
- Accept approvals from the web, signed with a passkey, or from the CLI,
  signed with an SSH key. The private key never sits on the controller, so
  the controller cannot approve on its own.
- Let the agent fetch its desired state and run a plan with mise bootstrap
  (`mise bootstrap plan`). The agent reports drift. It applies only a plan
  that a person approved.
- Report agent identity, version, health, and drift over
  [OpAMP](https://github.com/open-telemetry/opamp-spec) over HTTP.
- Enroll agents inside a private network. A single-use install token is
  traded for a revocable agent credential.
- Target Arch Linux (including Omarchy), macOS with Homebrew, and Ubuntu.

## What sewa will not do

- Hold any network's configuration, inventory, hostnames, or addresses in
  this repository. These live in the controller's database.
- Manage users' personal dotfiles or per-user tools. Those stay in each
  user's own dotfiles.
- Store secrets.
- Apply changes without approval by default.
- Replace the package managers. sewa drives mise, pacman, Homebrew, and apt
  through mise bootstrap.

## Development

Create commits with `my-commit` (conventional commits). Pull requests merge
with rebase-and-merge. See [AGENTS.md](AGENTS.md) and
[docs/PRINCIPLES.md](docs/PRINCIPLES.md).
