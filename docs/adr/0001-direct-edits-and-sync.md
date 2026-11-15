---
status: accepted
date: 2026-11-15
supersedes: []
superseded_by: null
tags: [desired-state, approval, api]
---

# Direct edits, and a manual sync to apply them

Edits went into one draft change, which an admin approved as a whole for
every host it touched. That mixed two decisions: what the desired state
is, and when hosts get it. Now admins edit roles and hosts directly, each
save shows a diff of the items and asks to confirm, and a separate, manual
sync applies the desired state to the hosts the admin picks. The passkey
signature stays where it was: an agent applies only plans an admin signed.

## Considered options

- **Keep the draft, sync on approval.** Rejected: one draft blocks
  unrelated edits, and a host cannot be synced alone.
- **Apply on save.** Rejected: an edit would reach every host at once,
  without a signed plan.
- **Edit directly, sync in steps (pick hosts, read plans, sign, apply).**
  Chosen.

## Consequences

- Each host has one config, `current`, which changes on every save. The
  agent plans it as soon as it changes, so a sync reads plans that exist
  already, and a host shows drift from the moment an edit lands.
- The approval keeps its wire format. Its `change_id` field holds the sync
  id, and the signed text still says `change <id>`, because installed
  agents read that format. A new field name would need every agent
  upgraded first.
- Migration `0005` drops changes, their edits and approvals, and the plans
  of change configs. Events keep their history without the change link.
- A drift revert is a sync of that host. Adopt writes the package to the
  role or the host directly.
- Roles cannot be deleted yet. Nothing in the redesign asked for it.
- Each item can name an OS (`linux` or `macos`). The controller filters
  items by `hosts.os`, so mise's own `os` option is not rendered.
