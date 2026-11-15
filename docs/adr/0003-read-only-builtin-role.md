---
status: accepted
date: 2026-11-15
supersedes: []
superseded_by: null
tags: [roles, desired-state, api]
---

# The built-in role is read-only

ADR 0002 let admins add packages to the built-in `bootstrap` role. An admin
edit there reaches every host and can break what sewa needs to run, so sewa
now manages the role alone: any save of it, and any adopt into it, returns
`409` `builtin_role`, and the web app shows its items without edit
controls. Admins put their own packages in other roles. This amends 0002.
