# Kiso in sewa

The web app follows Kiso. This file records only what Kiso leaves to the
product. The rules live in the installed contracts. If this file and a
contract disagree, the contract wins and this file has a bug.

## Contracts

- Package: `@momoi-labs/kiso-react` 0.31.1 and `@momoi-labs/kiso` 1.9.1 in
  `web/`.
- Contracts: `web/node_modules/@momoi-labs/kiso/kiso/`. Start at `AGENTS.md`.

## Paths

| What | Where |
| --- | --- |
| Screens | `web/src/hosts/`, `web/src/desired/`, `web/src/sync/`, `web/src/passkeys/` |
| Product components | `web/src/components/`, `web/src/hosts/`, `web/src/desired/`, `web/src/sync/`, `web/src/passkeys/` |
| Product CSS | `web/src/product.css`: only what Kiso leaves to the product: a LogView height, the tinted brand slot, the numbered form steps, the host picker layout, the bar height and bottom line the sidebar header and the top bar share, the gap above the first nav item (`--spacing-sm`, as the page body), the role item tables (the full-width role form and the phone cards, see Exceptions), the sync plan tables, the wrapping step row, and the sync run's tab row, status filter, host list, detail and activity feed. `web/src/components/SewaMark.css` holds the logo colors. `web/src/components/ErrorPage.css` sizes the error illustration. |

## Product components

| Component | Use |
| --- | --- |
| `Shell` | The application shell with the Hosts, Roles, Sync and Passkeys destinations and the theme selector. Its brand slot is tinted with the accent surface and shows the bold name over the instance address from `GET /api/instance`. |
| `SewaMark` | The sewa emblem in the shell's brand. The brand kit is in `docs/brand/`. |
| `HostStatusBadge` | A host's status: waiting, connected, disconnected, or disabled. Only the disconnected dot pulses. |
| `ErrorAlert` | An API failure, with what happened and what to do. |
| `ErrorPage` | The screen when a page's data fails to load with `not_admin`, a 5xx, or no answer: a brand illustration, the code, a title, one line, and one action. It fills the page with no shell, and the root route renders it. |
| `Unknown` | A missing value: a muted dash with a tooltip that says why. |
| `SkeletonRows` | Table rows while a list loads. |
| `NotFound` | The screen for an unknown path. |
| `AddHostDialog` | Creates a host and shows its install command once. |
| `HostActions` | The Disable or Enable button and the Delete button in a host page's header. Disable acts at once and shows a toast. Enable opens the install command in a Dialog (`InstallStep`). Delete opens an AlertDialog that names what goes with the host, with Cancel and a destructive "Delete host", then returns to the list. |
| `AddRoleDialog` | Creates an empty role, then opens it. |
| `ItemTable` | A role's items as tables, one per source (OS default, Homebrew, Cask, pacman, AUR, mise tools), each in a Disclosure with its count. A Search filters the rows. The add field is a kind Select and a ChipInput with no chips, which keeps the prefix syntax and the suggestions from the other roles. Version (Input) and OS (Select) change in the row. Read-only for the built-in role. |
| `SaveRoleDialog` | The items a role save adds, changes, and removes, and the button that saves them. |
| `ItemForm` | Adds a host adjustment: a package or a mise tool with a version. |
| `HostPicker` | The Pick hosts step of a new sync: a filter column (OS, Status, Role, each value a Checkbox with its host count; a host whose agent has not reported its OS counts as Unknown; values in a group are OR, groups are AND), a Search, the buttons All, All Linux, All macOS and None, an "X of Y selected" counter, and a sortable table of Checkbox, Host, OS, Roles and Status (`HostStatusBadge`). The table is nested in a panel, so it has `data-shadow-style="none"`. The search, the filters, the buttons and the header Checkbox act only on the hosts shown. Below 1024 px the filters sit above the table. |
| `PlanByPackage` | The Plans step of a sync by package: Install, Update and Remove tables, each under a heading with an icon (Package, Manager, Version or from and to, Hosts), each with its count and hidden when empty. A row merges the hosts that make the same change with the same versions. The Hosts cell says "all (N)", the names, or "build-1, build-2 +6" with a Popover that lists every host. A Search and an "only host" Select narrow the rows. Hosts with no plan to sign go in one line at the end, by reason. A plan error shows in an Alert per host: error when the plan failed, warning when the other changes still plan. |
| `PlanView` | A mise bootstrap plan as a table, removals first and in danger. |
| `PasskeyError` | A failed passkey prompt, or an API failure through `ErrorAlert`. |
| `AddPasskeyDialog` | Names a passkey, then lets the browser create it. |
| `AdoptDialog` | Adds an undeclared package to a role or to the host's adjustments. |

## Product rules

- The appearance attributes on `<html>` in `web/index.html` are the
  product's Kiso config. Change them there only.
- `data-theme` is always `light` or `dark`. `web/public/theme.js` sets it
  before first paint from the saved choice, or from the OS mode when the
  choice is "system".
- Panels come from `Card` and `TableFrame`, which render the panel shadow.
- Only a top-level panel keeps the soft shadow. A panel inside another
  panel has none: a table inside a card, such as the role item tables and
  the sync plan tables, and a panel inside tabs. Set
  `data-shadow-style="none"` on the nested `TableFrame`, `Card` or `Tabs`.
  `frame="none"` alone does not remove the shadow, because a later, more
  specific Kiso rule still sets it.

## Pattern choices

| Screen | Pattern |
| --- | --- |
| Hosts (`/`) | List-detail, separate screens: the list screen. |
| Add a host | CRUD: a small record created in a dialog. |
| Host (`/hosts/$hostId`) | List-detail, separate screens: the detail screen. |
| Roles (`/roles`) | List-detail, separate screens: the list screen. |
| Disable, enable and delete a host | Destructive actions: a Button pair in the page header. Delete confirms in an AlertDialog. Enable shows the new install command in a Dialog. A disabled host has an info Alert under the header. |
| Add a role | CRUD: a small record created in a dialog. |
| Role (`/roles/$roleId`) | Configuration form in numbered steps (`.form-steps`): Role, then Items in an `ItemTable`. A step with complete fields shows a check. FormActions is sticky and shows unsaved edits. Hosts get roles on the host page, not here. The built-in role shows its items read-only, with one line that sewa manages it, and no FormActions. |
| Save a role | Confirmations: a dialog with the item diff (added, changed, removed) and "Save". |
| Sync (`/sync`) | A StepList of the run to come: the edits since the last sync, then the `HostPicker`, then Plans, Sign and Apply as pending. Recent syncs sit in a table below. |
| Sync run (`/sync/$syncId`) | Before signing: a StepList of the run: Pick hosts (one summary line, "N hosts: X Linux, Y macOS, Z disconnected", with "Show all" for the names), Plans (a `PlanByPackage`, not one plan per host), Sign. A run done with no plan to sign shows a warning Alert and keeps the Plans step open. After signing: one Card. Its top holds the finished Alert, a one-line `Steps` (Pick hosts, Plans, Sign, Apply) and the "Apply progress" `Progress` bar with its count. Below, full-width `Tabs` "Activity" and "Hosts", each with a count Badge, share a tab row with the filters: Search (Activity only), a "Status" filter, a "Newest first" or "Oldest first" segmented `RadioGroup` (Activity only) and "Clear filters", disabled while no filter is set. Kiso has no multi-select, so the Status filter is a `Popover` with a Checkbox group (Failed, Applying, Waiting, Applied, In sync, Skipped, each with its host count), as the ChipInput contract advises for several values from a short list. The trigger reads "Status", "Status: Failed" or "Status: N selected", with a count Badge. The filter applies to both tabs, by the host's current status. Activity, the default tab, is a full-width feed of signed, started, step, applied and failed events (time, host, event) in its own scroll box, newest first. When new events arrive while the user is scrolled away, a "N new" button jumps back to them; oldest first follows the bottom until the user scrolls up. A host row opens the Hosts tab on that host, with a "Back to activity" button. Hosts is master-detail: a host list (Search, ordered failed, replan, applying, waiting, applied, in sync, skipped) and the selected host's detail (status, plan, log, Retry). An applying host has a pulsing success Dot and StatusBadge with the label "Applying", a line with its current step and start time (or a warning when it stops reporting), and its live log. A signed host's detail has a StepList: Signed, Checked, Native packages (only when reported), mise, Reported; an agent that sends no progress shows only Signed and Reported. The first failed host is selected by default. On a phone everything is one column, the filters wrap and the detail opens inline under the tapped row. |
| Passkeys (`/passkeys`) | CRUD: a list, with each passkey created in a dialog. |
| Sign a sync | The Sign step: the plans above it, then "Sign and sync" with a passkey. A failed host has "Retry", which signs its plan again. |
| Adopt a package | CRUD: a small edit made in a dialog. |
| Revert and Ignore drift | Plain actions on the host's Drift tab. Revert starts a sync of that host, which still needs a signature; Ignore only hides. |
| Error page | Permission denied (403 `not_admin`): a locked screen that replaces the whole page, shell included, with the hatch over the viewport and one centered column, with "Check again". Errors (5xx, no answer): the same composition with "Retry". Other load errors, and a failed refresh of data already on screen, keep `ErrorAlert`. |

## Exceptions

- The shell's brand shows the sewa emblem with `SewaMark` in place of a
  BrandMark, 32 px high, with no fill behind it. Kiso 1.9's BrandMark holds a
  letter or a 14 px line icon on the primary fill, and the emblem cannot be
  read at that size. Its colors are the brand's, in `SewaMark.css`, not Kiso
  tokens, so it keeps its colors with any accent. Kiso needs a BrandMark for
  a product logo.
- `ErrorPage` is a full page: the `hatch` fills the viewport and one
  column (`.empty` with Kiso's title, description and actions) holds a brand
  illustration, about 192 px high. It uses that composition for 5xx errors
  too, and it is not inside an EmptyState panel. Kiso prefers a
  small icon in EmptyState and an Alert for errors. The brand asks for the
  illustrated page when the whole screen cannot load. The page keeps Kiso's
  copy order (what, why, now), uses no danger color, and the 5xx page is an
  `alert` region. The illustrations are the brand's files in
  `web/src/components/error-art/`, inlined in the bundle, not Kiso tokens. Kiso has no illustration size token, so the
  height is `--spacing-4xl` times 3.
- The role form is as wide as the page. Kiso keeps `.form-page` at most
  720 px, and a role with many items needs the room for its Name, Manager,
  Version and OS columns. `product.css` lifts the limit for `.role-form`
  only.
- On a phone each row of a role's item tables is a card: the name and the
  remove button, then the manager, then the version and the OS. Kiso keeps
  table rows on a phone and drops columns, or scrolls sideways. Here every
  column is an input the person edits, so none can go, and the user asked
  for no sideways scroll at 390 px. The cards keep the table markup, so the
  column names stay in the accessibility tree. The sync plan tables use
  the same cards, with a "Hosts" label, so 20 hosts and long names fit.
- A StepList row wraps (`.step-row`), so a long step meta such as "16
  plans, 2 in sync, 2 offline, 1 failed" goes under the label on a phone.
  Kiso keeps `.step-meta` on one line, which scrolled the page sideways.

## Verify

1. In `web/`, run `npm ci`.
2. Run `npm run lint`, `npm test`, and `npm run build`. The build runs the
   typecheck first.
3. Build and run the controller (see [api.md](api.md#web-app)), then open
   `/` on its address in both themes and at 390px wide.
