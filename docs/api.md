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
| 401 | `token_invalid` | The install token is unknown, used, or expired. |
| 403 | `not_tailnet` | WhoIs does not know the caller as a tailnet node. |
| 403 | `not_admin` | The caller's login is not in `SEWA_ADMINS`. |
| 409 | `name_taken` | A host with this name exists. |
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
empty body. Each message records when the host was last seen.

## Events

The controller appends an event when a host is created, enrolls, connects,
and disconnects (`host.created`, `host.enrolled`, `host.connected`,
`host.disconnected`). The `events` table refuses updates and deletes.
