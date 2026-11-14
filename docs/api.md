# Controller API

The controller listens on one address on the tailnet (`SEWA_LISTEN`). Every
request is identified with Tailscale WhoIs through the local tailscaled
socket. There is no way to turn this check off.

## Configuration

| Variable | Meaning |
| --- | --- |
| `DATABASE_URL` | PostgreSQL connection string. |
| `SEWA_LISTEN` | Address and port to listen on, for example the controller's tailnet IP. |
| `SEWA_PUBLIC_URL` | Base URL that hosts use to reach the controller. It goes into the install command. |
| `SEWA_ADMINS` | Comma-separated Tailscale login names allowed to call `/api`. |
| `SEWA_AGENT_DIR` | Directory with the agent binaries, as built by `dev/agent-files.sh`. |
| `SEWA_TAILSCALE_SOCKET` | tailscaled socket. Default `/var/run/tailscale/tailscaled.sock`. |

## Errors

Errors from `/api` and `/v1/enroll` are JSON: `{"error": "<code>"}`.

| Status | Code | When |
| --- | --- | --- |
| 400 | `invalid_name` | The name is empty, longer than 63 characters, or has characters other than letters, digits, `-`, `_` and `.`. |
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
             "plan": {}, "error": null, "reported_at": "…", "stale": false },
  "undeclared": { "manager": "pacman", "packages": ["htop"], "explicit_count": 120,
                  "reported_at": "…" }
}
```

- `roles`, `adjustments`, and `items` are approved. The `proposed_` fields
  have the draft change applied, or are null without a draft.
- `drift` is the last plan of the approved state. Its `status` is
  `in_sync` (exit code 0), `drift` (2), or `error` (1). `stale` is true
  when the plan is of an older config.
- `undeclared` lists the packages the package manager says were installed
  explicitly that no approved item declares. It is null until the agent
  reports them.

## Desired state

A host's desired state is the items of its roles, with its own
adjustments on top. An item is a `package` (a mise bootstrap package key,
such as `jq` or `pacman:jq`) or a `tool` (a mise tool). Its version is
`latest`, a version, or, for a package, `absent`. When two roles of a host
declare the same item, the role whose name sorts last wins. An adjustment
`set`s an item's version on one host, or `remove`s an item its roles
declare.

Only approved state reaches the desired-state tables. Every edit goes into
the one draft change first. Approval is not built yet, so a change is a
draft or discarded.

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
   "closed_at": null, "edit_count": 2 }]
```

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
                        "plan": {}, "error": null, "reported_at": "…", "stale": false } }] }
```

`plan` is null until the agent reports one.

### `POST /api/changes/{id}/discard`

Closes a draft without applying it. Returns the change.

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
`mise -C <dir> bootstrap plan --json --detailed-exitcode`. It never applies a
plan. mise reads that one file and nothing else:

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

## Events

The controller appends an event when a host is created, enrolls, connects,
and disconnects (`host.created`, `host.enrolled`, `host.connected`,
`host.disconnected`). It also appends `role.created`, `change.created`,
`change.edited` (one per edit), and `change.discarded`, which name the role
or change and, for host edits, the host. `drift.detected` is appended once
per new drift: when a plan of `current` exits 2 with a plan hash other than
the last drift's. A plan that exits 0 ends the drift. The `events` table
refuses updates and deletes.
