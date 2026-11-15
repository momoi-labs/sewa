# Controller API

The controller listens on one address on the tailnet (`SEWA_LISTEN`). Every
request is identified with Tailscale WhoIs through the local tailscaled
socket. There is no way to turn this check off.

## Configuration

| Variable | Meaning |
| --- | --- |
| `DATABASE_URL` | PostgreSQL connection string. |
| `SEWA_LISTEN` | Address and port to listen on, for example the controller's tailnet IP. |
| `SEWA_PUBLIC_URL` | Base URL that hosts use to reach the controller. It goes into the install command. It is also the passkeys' relying party: its host is the RP id and the URL is the origin, so open the web app at this URL. Browsers allow passkeys only on `https` or `localhost`. |
| `SEWA_ADMINS` | Comma-separated Tailscale login names allowed to call `/api`. |
| `SEWA_AGENT_DIR` | Directory with the agent binaries, as built by `dev/agent-files.sh`. |
| `SEWA_TAILSCALE_SOCKET` | tailscaled socket. Default `/var/run/tailscale/tailscaled.sock`. |

## Errors

Errors from `/api` and `/v1/enroll` are JSON: `{"error": "<code>"}`.

| Status | Code | When |
| --- | --- | --- |
| 400 | `invalid_name` | A host or role name is empty, longer than 63 characters, or has characters other than letters, digits, `-`, `_` and `.`. A passkey name is empty, longer than 64 characters, or has control characters. |
| 400 | `unsupported_algorithm` | The passkey is not ES256 (COSE `-7`). |
| 400 | `invalid_passkey` | The passkey's key, credential id, authenticator data, or client data does not decode or check. |
| 400 | `unknown_challenge` | The registration challenge is unknown, used, older than five minutes, or of another login. |
| 400 | `wrong_origin` | The client data's origin is not `SEWA_PUBLIC_URL`. |
| 400 | `invalid_assertion` | The approval's assertion is not from a passkey of the signed-in user, or does not verify. |
| 400 | `invalid_body` | The body is not the expected JSON. |
| 400 | `invalid_item` | An item name or version is empty, longer than 200 characters, has control characters or outer spaces, a tool is `absent`, or the OS is not `linux` or `macos`. |
| 400 | `duplicate_item` | A role lists the same item twice. |
| 400 | `too_many_items` | A role has more than 500 items. |
| 400 | `invalid_edit` | A host edit has an invalid item. |
| 400 | `too_many_edits` | More than 500 host edits at once. |
| 400 | `unknown_role` | The edit names a role that does not exist, or the built-in role. |
| 400 | `no_hosts` | A sync picks no host. |
| 400 | `unknown_host` | A sync picks a host that does not exist or never enrolled. |
| 401 | `token_invalid` | The install token is unknown, used, or expired. |
| 403 | `not_tailnet` | WhoIs does not know the caller as a tailnet node. |
| 403 | `not_admin` | The caller's login is not in `SEWA_ADMINS`. |
| 404 | `not_found` | The host, role, or sync does not exist, or the path under `/api` is unknown. |
| 409 | `name_taken` | A host or role with this name exists. |
| 409 | `builtin_role` | A save or an adopt writes to the built-in role. sewa manages it, so it is read-only. |
| 409 | `passkey_exists` | The credential is saved already. |
| 409 | `no_passkey` | The signed-in user has no passkey to approve with. |
| 409 | `nothing_to_approve` | No host of the sync has changes to sign, and no picked host is in sync. |
| 409 | `plans_changed` | A plan changed since the browser got the challenge. Get a new one. |
| 503 | `whois_unavailable` | tailscaled did not answer. |
| 500 | `internal` | Anything else. Details go to the controller log. |

## Hosts

Only admins call these.

### `POST /api/hosts`

Creates a host and a single-use install token that expires after one hour.
The controller keeps only the token's hash, so the response is the only
place the token appears.

```json
{ "name": "build-1" }
```

Response `201`:

```json
{
  "host": { "id": "…", "name": "build-1", "os": null, "status": "waiting", "created_at": "…", "enrolled_at": null, "last_seen_at": null, "agent": null },
  "install_command": "curl -fsSL 'https://controller.example/install' | sudo sh -s -- '<token>'",
  "token_expires_at": "…"
}
```

The admin does not pick the OS. A host's `os` is null until its agent
connects. The agent reports its OS in every report: `darwin` sets `os` to
`macos`, and `linux` sets it to `linux`. A report without a description, or
with another OS, leaves `os` as it is. The host's config follows `os`.

### `GET /api/hosts`

Lists hosts by name. Each host also has `roles`, a list of `{ "id", "name" }`
by role name. It leaves out the built-in role, which every host has.
`status` is:

- `waiting`: the host never enrolled.
- `connected`: the agent reported in the last 90 seconds.
- `disconnected`: otherwise.

`agent` is what the agent last described, or `null`:

```json
{ "version": "0.1.0", "hostname": "build-1", "os": "linux", "arch": "amd64", "user": "root" }
```

### `GET /api/hosts/{id}`

Returns one host, in the same shape as an item of `GET /api/hosts`, without
`roles`.

### `GET /api/hosts/{id}/events`

Returns the host's events, newest first by `at`, at most 100:

```json
[{ "id": 3, "kind": "host.connected", "at": "…", "data": {} }]
```

`at` is when the event happened. For a report the agent kept while it was
offline, that is the agent's time, and `data.received_at` says when the
controller got it.

### `GET /api/hosts/{id}/state`

Returns the host's desired state and what its agent last found:

```json
{
  "roles": [{ "id": "…", "name": "base" }],
  "adjustments": [{ "kind": "package", "name": "sl", "action": "remove" }],
  "items": [{ "kind": "package", "name": "jq", "version": "latest", "os": null }],
  "config": "# Rendered by the sewa controller. Do not edit.\n…",
  "health": "drift",
  "drift": { "status": "drift", "config_hash": "…", "plan_hash": "…", "exit_code": 2,
             "plan": {}, "error": null, "reported_at": "…", "received_at": null,
             "stale": false, "ignored": false },
  "undeclared": { "manager": "pacman", "packages": ["htop"], "ignored_count": 0,
                  "explicit_count": 120, "reported_at": "…" }
}
```

- `roles` are the roles an admin gave the host. Every host also has the
  built-in role, which is not in the list.
- `items` are the host's merged items, and `config` is what the host gets
  for them.
- `drift` is the last plan of `current`. Its `status` is `in_sync` (exit
  code 0), `drift` (2), or `error` (1). `stale` is true when the plan is
  of an older config. `ignored` is true when Ignore hid this drift plan.
  `reported_at` is when the agent planned, by its clock. `received_at` is
  when the controller got the plan if it came more than 90 seconds late,
  from the agent's outbox; otherwise null.
- `undeclared` lists the packages the package manager says were installed
  explicitly that no item of the host declares, without the ones Ignore
  hid (`ignored_count`). It is null until the agent reports them.

### `POST /api/hosts/{id}/edits`

Changes the host's roles and adjustments in one transaction:

```json
{ "edits": [
  { "op": "host_role_add", "role": "…" },
  { "op": "host_role_remove", "role": "…" },
  { "op": "host_item_set", "kind": "tool", "name": "node", "version": "22" },
  { "op": "host_item_remove", "kind": "package", "name": "jq" },
  { "op": "host_item_reset", "kind": "package", "name": "jq" }
] }
```

`host_item_reset` drops the host's adjustment, so its roles decide again.
Returns the host's state. `400` `invalid_edit`, `too_many_edits` (more
than 500), or `unknown_role`; the built-in role counts as unknown, since
every host has it. Records `host.roles_set` with the role names added and
removed, and `host.items_set` with the item edits. The host gets the
change at its next [sync](#syncs).

### `POST /api/hosts/{id}/drift/adopt`

Takes `{"package": "htop", "role": "<role id>"}`, or `"role": null` for
the host's own adjustments. Adds the package at version `latest`. A role
that has the package keeps its version. Returns `204`. Records
`drift.adopted`. `409` `builtin_role` for the built-in role. To revert a drift, sync the host.

### `POST /api/hosts/{id}/drift/ignore`

Hides the host's drift plan and its undeclared packages as they are now.
The drift shows again when its plan hash changes, and a package shows
again when it is not in the hidden list. Returns `204`. Records
`drift.ignored`.

## Desired state

A host's desired state is the items of the built-in role, then of its
roles, then its own adjustments. An item is a `package` (a mise bootstrap
package key, such as `jq` or `pacman:jq`) or a `tool` (a mise tool). Its
version is `latest`, a version, or, for a package, `absent`. An item can
name an `os`, `linux` or `macos`: only hosts of that OS get it, and null
means every OS. When two roles of a host declare the same item, the role
whose name sorts last wins. An adjustment `set`s an item's version on one
host, or `remove`s an item its roles declare.

Edits change the desired state at once. No host changes until an admin
syncs it: see [syncs](#syncs).

### The built-in role

The role named `bootstrap` holds what sewa needs on a host. It starts
with one item, the package `mise`. Every host has it. sewa manages it, so
it is read-only: a save or an adopt into it returns `409` `builtin_role`,
and it cannot be deleted. An unprefixed package of the built-in role goes to the host's package
manager: `pacman:` on Linux, `brew:` on macOS. So a fresh Mac gets
`brew:mise`, which the agent installs before any mise item (see
[native packages](#native-packages)). The controller creates the role
when it migrates, and renames it back if its name changed.

### `GET /api/roles`

Returns every role, the built-in role first, then by name:

```json
[{ "id": "…", "name": "base", "builtin": false, "created_at": "…",
   "items": [{ "kind": "package", "name": "jq", "version": "latest", "os": null }],
   "hosts": [{ "id": "…", "name": "build-1" }] }]
```

`hosts` lists the hosts that have the role. The built-in role lists every
host.

### `GET /api/roles/{id}`

Returns one role in the same shape.

### `POST /api/roles`

Takes `{"name": "base"}` and creates an empty role. Returns `201` with the
role. Roles cannot be deleted.

### `PUT /api/roles/{id}`

Replaces the role's name and items:

```json
{ "name": "base", "items": [
  { "kind": "package", "name": "jq", "version": "latest" },
  { "kind": "tool", "name": "node", "version": "24", "os": "macos" }
] }
```

Returns the role. `400` `invalid_name`, `invalid_item`, `duplicate_item`,
or `too_many_items` (more than 500); `409` `name_taken`, or `builtin_role`
for any save of the built-in role.
Records `role.saved` with the name and the items `added`, `removed`, and
`changed` (a new version or OS), and `renamed_from` after a rename.

## Syncs

A sync brings the hosts an admin picks to their desired state. Each
agent plans its `current` config whenever it changes, so a sync reads the
plans the agents already sent. The admin signs the plans with a passkey,
and each agent checks the signature and applies its own plan.

A host's `step` in a sync is:

- `picked`: not signed yet. Its `plan` is the agent's last plan of
  `current`.
- `in_sync`: at the signature, its plan had nothing to do.
- `skipped`: at the signature, it had no plan to sign: it was offline,
  had not planned its config yet, or its plan failed.
- `waiting`: signed. Its agent has not reported yet.
- `applied`, `failed`, or `replan` (the plan changed before the agent
  applied it, so nothing ran): what the agent reported. A `failed` or
  `replan` host can be signed again.

The sync's `status` is `applying` while a host waits, `planning` while a
picked, connected host has no plan of its config, `ready` while a picked
host has changes to sign, and `done` otherwise.

### `POST /api/syncs`

Takes `{"hosts": ["<host id>", …]}` and starts a sync. Every host must be
enrolled. Returns `201` with the sync. `400` `no_hosts` or
`unknown_host`. Records `sync.created` per host.

### `GET /api/syncs`

Returns the 20 newest syncs:

```json
[{ "id": "…", "created_at": "…", "created_by": "…", "status": "done", "host_count": 3 }]
```

### `GET /api/syncs/edits`

Returns the edits since the last sync started, oldest first, at most 200:

```json
{ "since": "…",
  "edits": [{ "id": 12, "at": "…", "kind": "role.saved",
              "role": { "id": "…", "name": "base" }, "host": null, "data": {} }] }
```

`since` is null before the first sync. The edits are the `role.saved`,
`host.roles_set`, `host.items_set`, and `drift.adopted` events.

### `GET /api/syncs/{id}`

```json
{ "id": "…", "created_at": "…", "created_by": "…", "status": "applying",
  "hosts": [{ "id": "…", "name": "build-1", "os": "linux", "status": "connected",
              "last_seen_at": "…", "roles": [{ "id": "…", "name": "base" }],
              "step": "failed",
              "plan": { "config_hash": "…", "plan_hash": "…", "exit_code": 2,
                        "plan": {}, "error": null, "reported_at": "…", "received_at": null,
                        "stale": false },
              "result": { "plan_hash": "…", "log": "…", "reported_at": "…",
                          "received_at": null } }],
  "approvals": [{ "id": "…", "by": "…", "passkey": "Laptop", "created_at": "…", "host_count": 2 }] }
```

`plan` is null until the agent plans. `stale` is true when the plan is of
an older config, so a new plan is on its way. `result` is null until the
host is signed. `log` is the tail of the apply, at most 64 KiB.
`reported_at` and `received_at` work as in a plan.

### `POST /api/syncs/{id}/approval`

Returns what a signature signs now. The optional body
`{"hosts": ["<host id>"]}` limits it to those hosts, for example to retry
one failed host.

```json
{ "challenge": "<base64url>", "rp_id": "sewa.example.com",
  "allow_credentials": ["<base64url>"], "signed_text": "sewa approval v1\n…",
  "change_id": "<sync id>",
  "hosts": [{ "host_id": "…", "instance_uid": "…", "config_hash": "…", "plan_hash": "…" }] }
```

The hosts are those in step `picked`, `failed`, or `replan` with an agent
and a plan of their current config that has changes (exit code 2).
`change_id` holds the sync id: agents read this field. `signed_text` is:

```text
sewa approval v1
change <sync id>
host <host id> instance <agent instance uid> config <config hash> plan <plan hash>
```

with one `host` line per host, sorted by host id, each ending in a
newline. `challenge` is its SHA-256. `allow_credentials` lists the
signed-in user's passkeys. `409` `no_passkey`, or `nothing_to_approve`
when no host has changes to sign and no picked host is in sync. With
only hosts in sync, `hosts` is empty and the signature marks them.

### `POST /api/syncs/{id}/approve`

Takes the WebAuthn assertion of the challenge, as unpadded base64url, and
the same `hosts` as the approval call:

```json
{ "credential_id": "…", "authenticator_data": "…", "client_data_json": "…", "signature": "…",
  "hosts": null }
```

The controller checks it as an agent does, against the signed-in user's
passkeys, and checks that the plans did not change. It stores the
approval and the assertion, and sets each signed host to `waiting`.
Picked hosts in sync go to `in_sync`, and the other picked hosts to
`skipped`. Returns the sync. Records `sync.approved` per signed host.

## Instance

### `GET /api/instance`

Returns `{"address": "<SEWA_PUBLIC_URL>"}`, which the web app shows in its
header.

## Passkeys

A passkey signs approvals. Every admin's passkeys are approver keys.

### `GET /api/passkeys`

Returns every passkey, oldest first:

```json
[{ "id": "…", "login": "…", "name": "Laptop", "credential_id": "<base64url>", "created_at": "…" }]
```

### `POST /api/passkeys/options`

Returns a registration challenge, valid once for five minutes:

```json
{ "challenge": "<base64url>", "rp": { "id": "sewa.example.com", "name": "sewa" },
  "user": { "id": "<base64url>", "name": "<login>" }, "algorithm": -7,
  "exclude_credentials": ["<base64url>"] }
```

### `POST /api/passkeys`

Takes the new credential, as the browser's `getPublicKey()`,
`getPublicKeyAlgorithm()`, and `getAuthenticatorData()` return it, with
bytes as unpadded base64url:

```json
{ "name": "Laptop", "credential_id": "…", "public_key": "<SPKI DER>", "algorithm": -7,
  "authenticator_data": "…", "client_data_json": "…" }
```

The client data must be `webauthn.create` with a challenge from `options`
and the origin; the authenticator data must carry the RP id hash and the
user present flag. Returns `201` with the passkey. Records
`passkey.registered`. A new passkey reaches an agent only when its install
script runs again.

## Web app

The controller serves the web app at `/`. The app files hold no data, so
they are served like `/install`. The app reads everything from `/api`, which
checks WhoIs and `SEWA_ADMINS`. Paths that are not files return the app, so
its routes work on reload. Build the
app before you build the controller, because the controller embeds
`web/dist`:

```sh
cd web && npm ci && npm run build
```

Without a build, `/` says that the app is not built. To work on the app,
run `npm run dev` in `web/`. It proxies `/api` to `SEWA_CONTROLLER_URL`,
default `http://127.0.0.1:8080`.

## Install

### `GET /install`

Returns the install script. Run it as root with the token as its argument.
It downloads the agent for the machine's OS and architecture from
`/files/sewa-agent-<target>`, checks its SHA-256, installs it in
`/usr/local/bin`, enrolls, and starts the service:

- Linux: the systemd unit `sewa-agent.service`, as root.
- macOS: the launchd daemon `sewa.agent`, as a user the script asks for.
  The default is the user who ran `sudo`.

On macOS, when Homebrew is missing, the script installs it for that user
before it enrolls, with Homebrew's own non-interactive installer. The
agent needs brew for the [native packages](#native-packages), and never
installs Homebrew itself. The script prints each step, and stops with what
to do when a requirement is missing:

- the Mac must be Apple Silicon;
- the user must be an admin;
- the Xcode command line tools must be installed (`xcode-select
  --install`).

The installer refuses root and asks for sudo itself, so the script lets
the user run sudo without a password in `/etc/sudoers.d/sewa-homebrew`
while it runs, and removes that file afterwards, also when it fails.

The script holds every passkey of the moment, with the RP id and origin,
and pipes them into `sewa-agent pin`, which stores them in
`<state dir>/approvers.json` with mode 0600. Running the script again pins
the keys of that moment. Nothing else adds keys: the controller cannot
send keys over OpAMP. An agent with no pinned key applies nothing.

### `GET /files/<name>`

Returns an agent binary.

## Agents

### `POST /v1/enroll`

Called from a tailnet node with `{"token": "<install token>"}`. Spends the
token, revokes the host's earlier credentials, and returns a new agent
credential as `text/plain`. `sewa-agent enroll` stores it with mode 0600.

### `POST /v1/opamp`

OpAMP over HTTP. The body is a protobuf `AgentToServer`, sent with
`Authorization: Bearer <credential>`. The reply is a protobuf
`ServerToAgent`. A missing, unknown, or revoked credential gets `401` with an
empty body. Each message records when the host was last seen, and
`health.status` when the agent sends it.

#### Remote config

The controller sends each host its desired state as `remote_config`: one
file of the config map, `current`, with content type `application/toml`.
It changes as soon as an admin edits a role or the host, and the agent
plans it, but applies it only with a signed sync.

`config_hash` is the SHA-256 of the name and the hex SHA-256 of the body.
The controller sends `remote_config` only when the hash differs from the
agent's `remote_config_status.last_remote_config_hash`, so an agent that
was offline gets the config on its next message.

The agent writes the config to `<state dir>/configs/current/mise.toml`,
without its [native packages](#native-packages), and runs
`mise -C <dir> bootstrap plan --json --detailed-exitcode`. It applies only
an approval (see below). mise reads that one file and nothing else:

| Variable | Value | Keeps out |
| --- | --- | --- |
| every inherited `MISE_*` | dropped | `MISE_ENV`, other config paths |
| `MISE_SYSTEM_CONFIG_DIR` | an empty agent dir | `/etc/mise/config.toml`, `/etc/mise/conf.d/*` |
| `MISE_CONFIG_DIR`, `MISE_GLOBAL_CONFIG_FILE` | the empty agent dir | `~/.config/mise/config.toml` |
| `MISE_CEILING_PATHS` | `<state dir>/configs` | any `mise.toml` above the config |
| `MISE_TRUSTED_CONFIG_PATHS` | `<state dir>/configs` | the trust prompt |

`--mise` or `SEWA_MISE` names the mise binary. When it is a bare name
that is not in `PATH`, the agent also looks in `/opt/homebrew/bin` and
`/usr/local/bin`, where Homebrew puts it and launchd's `PATH` does not
reach. The agent plans a config
when it is new or its body changed, and plans `current` again every 24
hours. It reports health with `status` `in_sync` (exit code 0), `drift`
(2), or `error` (1).

#### Native packages

The agent installs some packages with the OS package manager itself,
without mise, so a host without mise still gets them:

| OS | Keys | Plan | Apply |
| --- | --- | --- | --- |
| macOS | `brew:<name>`, `brew-cask:<name>` | `brew list --formula -1`, `brew list --cask -1` | `brew install [--cask]`, `brew uninstall [--cask]` |
| Linux | `pacman:<name>` | `pacman -Qq` | `pacman -S --noconfirm --needed`, `pacman -Rns --noconfirm` |

Only keys at `latest` or with `state = "absent"` are native. A pinned
version, and `aur:` packages, stay with mise. A native item is `create`
when it is missing, `remove` when it is installed and should be absent,
and `noop` otherwise. Its resources come first in the plan's `resources`,
in mise's format, and `plan_hash` covers the whole plan. The agent applies
them before it runs mise, so the [built-in role](#the-built-in-role) gets
mise onto a fresh Mac in the same sync that applies the mise items.

When mise is missing:

- if a native item installs `mise`, the mise items are planned as
  `create`, and mise applies them right after the native step;
- otherwise the mise items are `unknown`, the plan carries the error
  `mise is missing; its items cannot be planned`, and they fail alone.
  When the native items have changes, the plan still exits 2, so they can
  be synced and applied; the result is `failed` with
  `mise is missing; its items were not applied` in its log.

When brew is missing on macOS, the native items are `unknown`, the plan
exits 1 with the error `Homebrew is missing; rerun the install script`,
and nothing else is planned on top of them.

#### Custom message `io.sewa.plan`

Both sides list `io.sewa.plan` in `custom_capabilities`. The agent sends
each plan as a custom message with capability `io.sewa.plan`, type `plan`,
and this JSON as `data`:

```json
{
  "config": "current",
  "config_hash": "<hex SHA-256 of the config body>",
  "plan_hash": "<hex SHA-256 of the plan>",
  "exit_code": 2,
  "plan": { "resources": [], "summary": {} },
  "error": null,
  "explicit": { "manager": "pacman", "packages": ["git", "jq"] },
  "at": 1767261600000
}
```

- `plan` is mise's JSON output, or null when mise printed none. With
  native packages it is the combined plan, and `plan_hash` is the SHA-256
  of its compact JSON. Without them, `plan_hash` is the SHA-256 of mise's
  output as printed.
- `explicit` comes only with `current`. It lists `pacman -Qqe` on Arch
  Linux, `apt-mark showmanual` on Debian and Ubuntu, or `brew leaves` on
  macOS.
- `at` is when the agent planned, in Unix milliseconds by its clock. The
  controller dates the plan with it, but never later than when it got it.
- The controller keeps the last plan of `current`. It drops plans of other
  configs, which agents from before syncs may send, and plans older than
  the one it keeps.

#### Custom message `io.sewa.approval`

Both sides list `io.sewa.approval` in `custom_capabilities`. On each reply
the controller sends the host's oldest approval it has not reported on, as
type `approval` with the approval as JSON `data`:

```json
{ "approval_id": "…", "change_id": "<sync id>",
  "hosts": [{ "host_id": "…", "instance_uid": "…", "config_hash": "…", "plan_hash": "…" }],
  "assertion": { "credential_id": "…", "authenticator_data": "…",
                 "client_data_json": "…", "signature": "…" } }
```

The reply carries any new `remote_config` too, and the agent stores the
config first. Then the agent:

1. verifies the assertion against its pinned keys: the credential is
   pinned, the client data is `webauthn.get` with the pinned origin and
   the SHA-256 of the approval's `signed_text` as challenge, the
   authenticator data has the RP id hash and the user present and user
   verified flags, and the ES256 signature verifies;
2. finds its own line by its instance UID;
3. checks that its `current` config has the signed config hash;
4. plans `current` again and checks that the plan hash is the signed one.

It applies only when every check passes, with
`mise -C <dir> bootstrap --yes --only packages,tools` and the same
isolation as the plan, then plans `current` again. It answers once per
approval with type `result`:

```json
{ "approval_id": "…", "change_id": "…", "status": "applied", "plan_hash": "…", "log": "…",
  "at": 1767261600000 }
```

`status` is `applied`, `failed` (a refused approval, or mise failed;
`log` says why), or `replan` (the config or the plan changed; nothing ran).
There is no rollback. One host failing does not stop the others. `at` is
when the agent finished, as in a plan.

#### Offline

The agent keeps its last configs, the approvals it answered, and its
reports in its state dir, so it works on while the controller is out of
reach. It plans `current` when it starts and every 24 hours, and applies
only an approval it holds and verifies.

Each plan and result goes into `<state dir>/outbox` before the agent
sends it, one file per message. The agent sends the oldest one with each
message and drops it once the controller answers, so they arrive in order,
after a restart too. Past 8 MiB the outbox drops its oldest plans first;
results go only when nothing else is left.

The agent writes `applying.json` before it runs `mise bootstrap` and
removes it once the result is in the outbox. If it finds the file when it
starts, mise stopped mid-apply, and it reports `failed` for that approval
instead of applying it again.

## Events

The controller appends an event when a host is created, enrolls, connects,
and disconnects (`host.created`, `host.enrolled`, `host.connected`,
`host.disconnected`). When a host speaks after more than 90 seconds of
silence, it appends `host.offline` dated from the start of the silence,
with `data.from` and `data.until`. It also appends `role.created` and `role.saved`, which name the
role, and `host.roles_set` and `host.items_set` for host edits. `drift.detected` is appended once
per new drift: when a plan of `current` exits 2 with a plan hash other than
the last drift's. A plan that exits 0 ends the drift. Syncs add
`sync.created` and `sync.approved` (per host), and the agents' `plan.applied`,
`apply.failed`, and `apply.replan`, which name the sync. Passkeys add
`passkey.registered`, and drift actions `drift.adopted` and
`drift.ignored`. Events from before syncs keep their old kinds, such as
`change.approved`. The `events` table refuses updates and deletes.
