---
status: accepted
date: 2026-11-16
supersedes: []
superseded_by: null
tags: [hosts, events, api]
---

# Disable keeps a host's history, delete removes it

Admins had no way to stop or remove a host, so test hosts went out of the
database by hand. Disable revokes the host's credentials and drops it from
syncs, and keeps its history. Enable issues a new install token, because
the old credentials stay revoked. Delete works on any host and removes what
belongs only to it. Delete is the one case where events leave `events`,
which was append-only until now: the migration lets a delete of events pass
only inside the transaction of a host delete.

## Considered options

- **Delete only after disable.** Rejected: a second step with no gain, since
  the confirmation in the web app already guards the delete.
- **Keep the events of a deleted host.** Rejected: `events.host_id` points
  at the host, and events with no host are the ones that never had one.
  A host that is gone should leave nothing readable under its name.
- **Un-revoke the old credentials on enable.** Rejected: a credential that
  leaked before the disable would work again.

## Consequences

- A sync that covers only the deleted host goes with its approvals and
  events. A sync that covers other hosts stays, with its approvals. An
  approval is a signature over a list of hosts that the controller does not
  edit, so the deleted host's id stays in that list.
- A revoked credential stays as a record while its host exists, and goes
  with the host.
