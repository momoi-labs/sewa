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
| 400 | `invalid_os` | The OS is not `linux` or `macos`. |
| 400 | `invalid_body` | The body is not the expected JSON. |
| 400 | `invalid_item` | An item name or version is empty, longer than 200 characters, has control characters or outer spaces, or a tool is `absent`. |
| 400 | `unknown_role` | The edit names a role that does not exist. |
| 400 | `unknown_host` | The edit names a host that does not exist. |
| 401 | `token_invalid` | The install token is unknown, used, or expired. |
| 403 | `not_tailnet` | WhoIs does not know the caller as a tailnet node. |
| 403 | `not_admin` | The caller's login is not in `SEWA_ADMINS`. |
| 404 | `not_found` | The host, role, or change does not exist, or the path under `/api` is unknown. |
| 409 | `name_taken` | A host or role with this name exists. |
| 409 | `not_draft` | The change is not a draft. |
| 409 | `passkey_exists` | The credential is saved already. |
| 409 | `no_passkey` | The signed-in user has no passkey to approve with. |
| 409 | `not_approvable` | The change is discarded, or approved with no host that needs a new approval. |
| 409 | `nothing_to_approve` | The change affects no host. |
| 409 | `plans_not_ready` | An affected host has no agent, no plan of the config it would get, or a plan that failed. |
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
{ "name": "build-1", "os": "linux" }
```

Response `201`:

```json
{
  "host": { "id": "…", "name": "build-1", "os": "linux", "status": "waiting", "created_at": "…", "enrolled_at": null, "last_seen_at": null, "agent": null },
  "install_command": "curl -fsSL 'https://controller.example/install' | sudo sh -s -- '<token>'",
  "token_expires_at": "…"
}
```

### `GET /api/hosts`

Lists hosts by name. `status` is:

- `waiting`: the host never enrolled.
- `connected`: the agent reported in the last 90 seconds.
- `disconnected`: otherwise.

`agent` is what the agent last described, or `null`:

```json
{ "version": "0.1.0", "hostname": "build-1", "os": "linux", "arch": "amd64", "user": "root" }
```

### `GET /api/hosts/{id}`

Returns one host, in the same shape as an item of `GET /api/hosts`.

### `GET /api/hosts/{id}/events`

Returns the host's events, newest first, at most 100:

```json
[{ "id": 3, "kind": "host.connected", "at": "…", "data": {} }]
```

### `GET /api/hosts/{id}/state`

Returns the host's desired state and what its agent last found:

```json
{
  "roles": [{ "id": "…", "name": "base" }],
  "proposed_roles": null,
  "adjustments": [{ "kind": "package", "name": "sl", "action": "remove" }],
  "proposed_adjustments": null,
  "items": [{ "kind": "package", "name": "jq", "version": "latest" }],
  "config": "# Rendered by the sewa controller. Do not edit.\n…",
  "health": "drift",
  "drift": { "status": "drift", "config_hash": "…", "plan_hash": "…", "exit_code": 2,
             "plan": {}, "error": null, "reported_at": "…", "stale": false,
             "ignored": false, "revert_change": null },
  "undeclared": { "manager": "pacman", "packages": ["htop"], "ignored_count": 0,
                  "explicit_count": 120, "reported_at": "…" }
}
```

- `roles`, `adjustments`, and `items` are approved. The `proposed_` fields
  have the draft change applied, or are null without a draft.
- `drift` is the last plan of the approved state. Its `status` is
  `in_sync` (exit code 0), `drift` (2), or `error` (1). `stale` is true
  when the plan is of an older config. `ignored` is true when Ignore hid
  this drift plan. `revert_change` is the host's open revert change, or
  null.
- `undeclared` lists the packages the package manager says were installed
  explicitly that no approved item declares, without the ones Ignore hid
  (`ignored_count`). It is null until the agent reports them.

### `POST /api/hosts/{id}/drift/revert`

Opens a revert change for the host: a change with no edits whose approval
applies the host's approved state again, with its plan of `current`, which
fixes the drift. Returns `201` with the change, or `200` with the open
revert change when there is one. Records `drift.reverted`.

### `POST /api/hosts/{id}/drift/adopt`

Takes `{"package": "htop", "role": "<role id>"}`, or `"role": null` for
the host's own adjustments. Adds the package at version `latest` as an
edit of the draft change. Returns `201` with the draft. Records
`drift.adopted`.

### `POST /api/hosts/{id}/drift/ignore`

Hides the host's drift plan and its undeclared packages as they are now.
The drift shows again when its plan hash changes, and a package shows
again when it is not in the hidden list. Returns `204`. Records
`drift.ignored`.

## Desired state

A host's desired state is the items of its roles, with its own
adjustments on top. An item is a `package` (a mise bootstrap package key,
such as `jq` or `pacman:jq`) or a `tool` (a mise tool). Its version is
`latest`, a version, or, for a package, `absent`. When two roles of a host
declare the same item, the role whose name sorts last wins. An adjustment
`set`s an item's version on one host, or `remove`s an item its roles
declare.

Only approved state reaches the desired-state tables. Every edit goes into
the one draft change first. A change is a `draft`, `approved`, or
`discarded`. Approving a draft writes its edits into the approved state,
so it becomes each host's new `current`. Besides the one draft of edits,
each host can have one open revert change (see
[drift revert](#post-apihostsiddriftrevert)).

### `GET /api/roles`

Returns every role by name:

```json
[{ "id": "…", "name": "base", "created_at": "…",
   "items": [], "proposed_items": [{ "kind": "package", "name": "jq", "version": "latest" }],
   "hosts": [], "proposed_hosts": [{ "id": "…", "name": "build-1" }] }]
```

### `GET /api/roles/{id}`

Returns one role in the same shape.

### `POST /api/roles`

Takes `{"name": "base"}` and creates an empty role. An empty role changes no
host, so this needs no change. Returns `201` with the role.

### `GET /api/changes`

Returns every change, newest first:

```json
[{ "id": "…", "status": "draft", "created_at": "…", "created_by": "…",
   "closed_at": null, "edit_count": 2, "revert_host": null }]
```

`revert_host` is `{"id": "…", "name": "…"}` for a revert change.

### `POST /api/changes/draft/edits`

Adds one edit to the draft change, and creates the draft when there is
none. Returns `201` with the change. The body is one of:

```json
{ "op": "role_item_set", "role": "…", "kind": "package", "name": "jq", "version": "latest" }
{ "op": "role_item_remove", "role": "…", "kind": "package", "name": "jq" }
{ "op": "host_role_add", "host": "…", "role": "…" }
{ "op": "host_role_remove", "host": "…", "role": "…" }
{ "op": "host_item_set", "host": "…", "kind": "tool", "name": "node", "version": "22" }
{ "op": "host_item_remove", "host": "…", "kind": "package", "name": "jq" }
{ "op": "host_item_reset", "host": "…", "kind": "package", "name": "jq" }
```

`host_item_reset` drops the host's adjustment, so its roles decide again.

### `GET /api/changes/{id}`

Returns the change, its edits, and each host it affects. A draft lists
the hosts whose config it alters. A closed change lists the hosts that
planned it.

```json
{ "id": "…", "status": "draft", "…": "…",
  "edits": [{ "id": 1, "at": "…", "by": "…", "edit": { "op": "…" } }],
  "hosts": [{ "id": "…", "name": "build-1", "status": "connected",
              "current_config": "…", "proposed_config": "…",
              "plan": { "config_hash": "…", "plan_hash": "…", "exit_code": 2,
                        "plan": {}, "error": null, "reported_at": "…", "stale": false },
              "result": { "status": "failed", "plan_hash": "…", "log": "…", "reported_at": "…" } }],
  "approvals": [{ "id": "…", "by": "…", "passkey": "Laptop", "created_at": "…", "host_count": 2 }] }
```

`plan` is null until the agent reports one. `result` is null until the
change is approved for the host. Its `status` is `waiting` until the agent
reports, then `applied`, `failed`, or `replan`: the plan changed before
the agent applied it, so nothing was applied and the host needs a new
approval. `log` is the tail of `mise bootstrap`, at most 32 KiB.

### `POST /api/changes/{id}/discard`

Closes a draft without applying it. Returns the change.

### `POST /api/changes/{id}/approval`

Returns what an approval of the change signs now:

```json
{ "challenge": "<base64url>", "rp_id": "sewa.example.com",
  "allow_credentials": ["<base64url>"], "signed_text": "sewa approval v1\n…",
  "change_id": "…",
  "hosts": [{ "host_id": "…", "instance_uid": "…", "config_hash": "…", "plan_hash": "…" }] }
```

The hosts are each host whose config a draft changes, with its plan of
`change/<id>`; the one host of a revert, with its plan of `current`; or,
for an approved change, the hosts whose result is `replan`, with their
plan of `current`. `signed_text` is:

```text
sewa approval v1
change <change id>
host <host id> instance <agent instance uid> config <config hash> plan <plan hash>
```

with one `host` line per host, sorted by host id, each ending in a
newline. `challenge` is its SHA-256. `allow_credentials` lists the
signed-in user's passkeys.

### `POST /api/changes/{id}/approve`

Takes the WebAuthn assertion of the challenge, as unpadded base64url:

```json
{ "credential_id": "…", "authenticator_data": "…", "client_data_json": "…", "signature": "…" }
```

The controller checks it as an agent does, against the signed-in user's
passkeys, and checks that the plans did not change. It stores the
approval and the assertion, writes a draft's edits into the approved
state, marks the change `approved`, and sets each host's result to
`waiting`. Returns the change. Records `change.approved` per host.

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

The controller sends each host its mise bootstrap configs as
`remote_config`. Each is a file of the config map, with content type
`application/toml`:

- `current`: the approved desired state. It has only a header until
  something is approved.
- `change/<change id>`: the draft change's proposed state, when the draft
  alters the host's config.

`config_hash` is the SHA-256 of the names and the hex SHA-256 of each body.
The controller sends `remote_config` only when the hash differs from the
agent's `remote_config_status.last_remote_config_hash`, so an agent that
was offline gets the configs on its next message.

The agent writes each config to `<state dir>/configs/<current |
change-<id>>/mise.toml` and runs
`mise -C <dir> bootstrap plan --json --detailed-exitcode`. It applies only
an approval (see below). mise reads that one file and nothing else:

| Variable | Value | Keeps out |
| --- | --- | --- |
| every inherited `MISE_*` | dropped | `MISE_ENV`, other config paths |
| `MISE_SYSTEM_CONFIG_DIR` | an empty agent dir | `/etc/mise/config.toml`, `/etc/mise/conf.d/*` |
| `MISE_CONFIG_DIR`, `MISE_GLOBAL_CONFIG_FILE` | the empty agent dir | `~/.config/mise/config.toml` |
| `MISE_CEILING_PATHS` | `<state dir>/configs` | any `mise.toml` above the config |
| `MISE_TRUSTED_CONFIG_PATHS` | `<state dir>/configs` | the trust prompt |

`--mise` or `SEWA_MISE` names the mise binary. The agent plans a config
when it is new or its body changed, and plans `current` again every 24
hours. It reports health with `status` `in_sync` (exit code 0), `drift`
(2), or `error` (1).

#### Custom message `io.sewa.plan`

Both sides list `io.sewa.plan` in `custom_capabilities`. The agent sends
each plan as a custom message with capability `io.sewa.plan`, type `plan`,
and this JSON as `data`:

```json
{
  "config": "change/…",
  "config_hash": "<hex SHA-256 of the config body>",
  "plan_hash": "<hex SHA-256 of mise's JSON output>",
  "exit_code": 2,
  "plan": { "resources": [], "summary": {} },
  "error": null,
  "explicit": { "manager": "pacman", "packages": ["git", "jq"] }
}
```

- `plan` is mise's JSON output, or null when mise printed none.
- `explicit` comes only with `current`. It lists `pacman -Qqe` on Arch
  Linux, `apt-mark showmanual` on Debian and Ubuntu, or `brew leaves` on
  macOS.
- The controller keeps the last plan of each config. It drops plans of
  unknown changes.

#### Custom message `io.sewa.approval`

Both sides list `io.sewa.approval` in `custom_capabilities`. On each reply
the controller sends the host's oldest approval it has not reported on, as
type `approval` with the approval as JSON `data`:

```json
{ "approval_id": "…", "change_id": "…",
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
{ "approval_id": "…", "change_id": "…", "status": "applied", "plan_hash": "…", "log": "…" }
```

`status` is `applied`, `failed` (a refused approval, or mise failed;
`log` says why), or `replan` (the config or the plan changed; nothing ran).
There is no rollback. One host failing does not stop the others.

## Events

The controller appends an event when a host is created, enrolls, connects,
and disconnects (`host.created`, `host.enrolled`, `host.connected`,
`host.disconnected`). It also appends `role.created`, `change.created`,
`change.edited` (one per edit), and `change.discarded`, which name the role
or change and, for host edits, the host. `drift.detected` is appended once
per new drift: when a plan of `current` exits 2 with a plan hash other than
the last drift's. A plan that exits 0 ends the drift. Approvals add
`passkey.registered`, `change.approved` (per host), `plan.applied`,
`apply.failed`, `apply.replan`, `drift.reverted`, `drift.adopted`, and
`drift.ignored`. The `events` table refuses updates and deletes.
