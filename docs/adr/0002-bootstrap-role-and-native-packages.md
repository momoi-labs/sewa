---
status: accepted
date: 2026-11-15
supersedes: []
superseded_by: null
tags: [agent, packages, install, desired-state]
---

# A built-in bootstrap role, and native packages without mise

A fresh Mac has no mise, and the agent planned everything with mise, so it
could plan nothing there. Now a built-in role named `bootstrap` holds what
sewa needs on a host, starting with mise. The agent installs `brew:`,
`brew-cask:` (macOS) and `pacman:` (Linux) packages at `latest` or
`absent` itself, before mise, so the same sync installs mise and then the
mise items. On macOS, the install script installs Homebrew; the agent
never does.

## Considered options

- **Let mise install mise.** Rejected: it cannot run before it exists.
- **Install mise in the install script.** Rejected: the version and its
  removal would sit outside the desired state, and admins could not add
  what sewa needs next.
- **A built-in role, applied first by the agent with the OS manager.**
  Chosen.
- **The agent installs Homebrew when it is missing.** Rejected: the
  installer needs an admin who can run sudo, and the Xcode command line
  tools. The agent runs unattended, and the install script runs with the
  user at the Mac.

## Consequences

- The role's name lives in one constant. It applies to every host, cannot
  be renamed or deleted, and holds only packages. An unprefixed package of
  it goes to `brew:` on macOS and `pacman:` on Linux.
- Native packages join mise's plan JSON, first, and the plan hash covers
  the combined plan, so the signature covers both parts.
- When mise is missing and a native item installs it, the mise items are
  planned as `create` and applied after the native step. mise applies the
  signed config, but its own plan is not signed in that run; the next plan
  shows what it did.
- A missing mise that nothing installs fails only the mise items. The
  native items still plan and apply.
- A missing Homebrew fails the native items with "Homebrew is missing;
  rerun the install script", and nothing is planned on top of them.
- `aur:` packages stay with mise, which needs an AUR helper that runs as
  a user. Only `pacman:` is native on Linux.
- The install script adds a temporary `NOPASSWD` sudo rule for the
  agent's user while Homebrew's installer runs, because the
  non-interactive installer uses `sudo -n`. It removes the rule on exit.
- Homebrew's installer supports Apple Silicon only, so the script stops on
  an Intel Mac and asks for Homebrew by hand.
