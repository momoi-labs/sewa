# Kiso in sewa

The web app follows Kiso. This file records only what Kiso leaves to the
product. The rules live in the installed contracts. If this file and a
contract disagree, the contract wins and this file has a bug.

## Contracts

- Package: `@momoi-labs/kiso-react` 0.32.0 and `@momoi-labs/kiso` 1.10.0 in
  `web/`.
- Contracts: `web/node_modules/@momoi-labs/kiso/kiso/`. Start at `AGENTS.md`.

## Paths

| What | Where |
| --- | --- |
| Screens | `web/src/hosts/`, `web/src/desired/`, `web/src/sync/`, `web/src/passkeys/` |
| Product components | `web/src/components/`, `web/src/hosts/`, `web/src/desired/`, `web/src/sync/`, `web/src/passkeys/` |
| Product CSS | `web/src/product.css`: only what Kiso leaves to the product: a LogView height, the logo, the bar stripe and the brand inset, the numbered form steps, the edge-to-edge EmptyState in a list Card and the FormActions corners in a form Card, the roles list on a host's desired state, the full-width new sync form, the host picker layout, the role item tables (the full-width role form and the phone cards, see Exceptions), the sync plan tables, the wrapping step row, and the sync run's tab row, status filter, host list, detail and activity feed. `web/src/components/ErrorPage.css` sizes the error illustration. |

## Product components

| Component | Use |
| --- | --- |
| `Shell` | The application shell (`shellLayout`, `collapsible`, `togglePlacement="header"`) with the Hosts, Roles, Sync and Passkeys destinations and the theme selector. Its brand is the sewa lockup. The page's host (`location.host`, so a non-default port shows) shows under it below 1024 px, and in the header breadcrumb as its root. |
| `HeaderBreadcrumb` | The path in the shell header: address, list page, record name. The record name comes from the same queries as the page, so it shares their cache. |
| `BrandSymbol`, `Lockup` | The sewa cat in the shell's brand: the symbol in the rail and the phone header, the lockup in the open sidebar and the drawer. Each renders the light and the dark drawing, and CSS shows the one for the theme. The files are in `web/src/brand/`, copies of `docs/brand/`. |
| `HostStatusBadge` | A host's status: waiting, connected, disconnected, or disabled. No dot pulses. |
| `ListFilters` | The `.list-filters` row above a list's TableFrame: a Search first, then the list's own filters (a Status Select on Hosts), then "Clear filters" while a filter is on. It shows even with a few rows. `NoMatchRow` is the row "No things match your filters.", `countOf` the footer "N of M things", and `ListEmpty` the empty list: a Card with only an EmptyState and no button. |
| `ErrorAlert` | An API failure, with what happened and what to do. |
| `ErrorPage` | The screen when a page's data fails to load with `not_admin`, a 5xx, or no answer: a brand illustration, the code, a title, one line, and one action. It fills the page with no shell, and the root route renders it. |
| `Unknown` | A missing value: a muted dash with a tooltip that says why. |
| `SkeletonRows` | Table rows while a list loads. |
| `NotFound` | The screen for an unknown path. |
| `AddHostDialog` | Creates a host and shows its install command once. |
| `HostActions` | The host page's header: the title, then one `Lifecycle` with the `HostStatusBadge` and the actions ("Host actions" when they collapse): Disable or Enable, and a ghost danger "Delete host". Disable acts at once and shows a toast. Enable opens the install command in a Dialog (`InstallStep`). Delete opens an AlertDialog that names what goes with the host, with Cancel and a destructive "Delete host", then returns to the list. |
| `ItemChips` | A role's items or a host's adjustments as two ChipInputs, Packages and Mise tools. A chip shows the name, the version (edit in place) and, on a role, the `os=` option. A chip that the server would refuse is invalid, and a ValidationMessage under the box says why. The suggestions come from the other roles. On a host, the version `absent` leaves a role item out. |
| `ItemTable` | The built-in role's items, read-only, as tables, one per source (OS default, Homebrew, Cask, pacman, AUR, mise tools), each in a Disclosure with its count. A Search filters the rows. |
| `SaveRoleDialog` | The items a role save adds, changes, and removes, and the button that saves them. |
| `HostPicker` | The Hosts step of a new sync: a filter column (OS, Status, Role, each value a Checkbox with its host count; a host whose agent has not reported its OS counts as Unknown; values in a group are OR, groups are AND), a Search, the buttons All, All Linux, All macOS and None, an "X of Y selected" counter, and a sortable table of Checkbox, Host, OS, Roles and Status (`HostStatusBadge`). The table is nested in a panel, so it has `data-shadow-style="none"`. The search, the filters, the buttons and the header Checkbox act only on the hosts shown. Below 1024 px the filters sit above the table. |
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
| Hosts (`/`) | List-detail, separate screens: the list screen. `ListFilters` above a TableFrame with `data-fill="false"`; an empty list is a `ListEmpty`. |
| Add a host | CRUD: a small record created in a dialog. |
| Host (`/hosts/$hostId`) | List-detail, separate screens: the detail screen. The Tabs use `sections`. The Desired state tab is a draft form in steps (Roles, then Adjustments in `ItemChips`) with a sticky FormActions: nothing saves until Save, which sends every edit at once. The Events table sits in a `TableFrame frame="none"`. |
| Roles (`/roles`) | List-detail, separate screens: the list screen. `ListFilters` above a TableFrame with `data-fill="false"`; an empty list is a `ListEmpty`. |
| Disable, enable and delete a host | Destructive actions: the Lifecycle actions in the page header. Delete confirms in an AlertDialog. Enable shows the new install command in a Dialog. A disabled host has an info Alert under the header. |
| New role (`/roles/new`) | Console form page: a Card (`form-page role-form`, `data-fill="false"`) with the Role and Items steps and a sticky FormActions with Cancel and "Create role". The role is created with its items. |
| Role (`/roles/$roleId`) | Configuration form in numbered steps (`.form-steps`): Role, then Items in `ItemChips`. The header has a `Lifecycle` with "Given to N hosts" or "Built in". There is no Delete role: the API has no endpoint for it. A step with complete fields shows a check. FormActions is sticky and shows unsaved edits. Hosts get roles on the host page, not here. The built-in role shows its items read-only, with one line that sewa manages it, and no FormActions. |
| Save a role | Confirmations: a dialog with the item diff (added, changed, removed) and "Save". |
| Sync (`/sync`) | Console form page "New sync": a Card (`form-page wide-form`, `data-fill="false"`) with the steps Edits since the last sync and Hosts (`HostPicker`), and a sticky FormActions with "N of M hosts selected", Cancel and "Plan N hosts". Recent syncs sit in a table below: a StatusBadge per sync (it pulses while planning or applying) and, while a sync runs, a StepBar "Apply progress" of its hosts with "x of y". |
| Sync run (`/sync/$syncId`) | Before signing: a StepList of the run: Pick hosts (one summary line, "N hosts: X Linux, Y macOS, Z disconnected", with "Show all" for the names), Plans (a `PlanByPackage`, not one plan per host), Sign. A run done with no plan to sign shows a warning Alert and keeps the Plans step open. After signing: one Card. Its top holds the finished Alert, a one-line `Steps` (Pick hosts, Plans, Sign, Apply) and the "Apply progress" `Progress` bar with its count. Below, full-width `Tabs` "Activity" and "Hosts", each with a count Badge, share a tab row with the filters: Search (Activity only), a "Status" filter, a "Newest first" or "Oldest first" segmented `RadioGroup` on one line, with its "Order" label hidden (Activity only) and "Clear filters", disabled while no filter is set. Kiso has no multi-select, so the Status filter is a `Popover` with a Checkbox group (Failed, Applying, Waiting, Applied, In sync, Skipped, each with its host count), as the ChipInput contract advises for several values from a short list. The trigger reads "Status", "Status: Failed" or "Status: N selected", with a count Badge. The filter applies to both tabs, by the host's current status. Activity, the default tab, is a full-width feed of signed, started, step, applied and failed events (time, host, event) in its own scroll box, newest first. When new events arrive while the user is scrolled away, a "N new" button jumps back to them; oldest first follows the bottom until the user scrolls up. A host row opens the Hosts tab with that host selected, scrolled into view and focused, and clears the host search. Hosts is master-detail: a host list (Search, ordered failed, replan, applying, waiting, applied, in sync, skipped) and the selected host's detail (status, plan, log, Retry). An applying host has a pulsing success Dot and StatusBadge with the label "Applying", a line with its current step and start time (or a warning when it stops reporting), and its live log. A signed host's detail has a StepList: Signed, Checked, Native packages (only when reported), mise, Reported; an agent that sends no progress shows only Signed and Reported. The first failed host is selected by default. On a phone everything is one column, the filters wrap and the detail opens inline under the tapped row. |
| Passkeys (`/passkeys`) | CRUD: a list, with each passkey created in a dialog. No page description: the empty state says why a passkey matters. |
| Sign a sync | The Sign step: the plans above it, then "Sign and sync" with a passkey. A failed host has "Retry", which signs its plan again. |
| Adopt a package | CRUD: a small edit made in a dialog. |
| Revert and Ignore drift | Plain actions on the host's Drift tab. Revert starts a sync of that host, which still needs a signature; Ignore only hides. |
| Error page | Permission denied (403 `not_admin`): a locked screen that replaces the whole page, shell included, with the hatch over the viewport and one centered column, with "Check again". Errors (5xx, no answer): the same composition with "Retry". Other load errors, and a failed refresh of data already on screen, keep `ErrorAlert`. |

## Exceptions

- The shell's brand shows the sewa cat in place of a BrandMark. Kiso's
  BrandMark holds a letter or a 14 px line icon on the primary fill, and the
  cat cannot be read at that size. The drawings keep their own colors, not
  Kiso tokens, so they keep them with any accent.
- Logo rules (from the Kiso console recipe, with sewa's drawings):
  - One drawing per theme (`mark.svg`, `mark-dark.svg`, `lockup.svg`,
    `lockup-dark.svg`). Both `<img>` render and CSS shows the one for
    `:root[data-theme]`, or for `prefers-color-scheme` when no theme is set.
  - The symbol is a square of `--size-control-sm`. The cat is 1469 by 1283, so
    it fits the square by width. The lockup is `--size-control-sm * 1283 / 1469`
    high (viewBox `24 24 4387.4 1283`), so its cat has the symbol's size.
    The cat does not move or resize when the sidebar collapses.
  - Kiso's brand inset assumes a `--size-control-xs` mark. The product sets
    `--app-shell-brand-inset` to `(--size-touch-min + 2 * --spacing-md - 1px -
    --size-control-sm) / 2`. The 1px is the rail border.
  - The sidebar bar has the accent surface and a 3px `--color-primary` stripe
    at the bottom, an inset box-shadow, because Kiso has no 3px token. The
    bar pads its bottom by the stripe, open and in the rail.
  - Open sidebar: the lockup alone. Rail: the symbol (`brandMark`). Phone
    header: the symbol, plus the address in mono up to 720 px. Drawer: the
    lockup with the address.
  - Below 1024 px the shell bar keeps one fixed height, as above 1024 px
    (`--app-shell-bar-height`), because Kiso's `shellLayout` sets it from
    1024 px only.
- The nav icons keep their x when the sidebar collapses, also during the
  animation. Kiso centers the icon in the rail, which follows the sidebar
  width and puts the icon 10px from the open position. `product.css` anchors
  `.nav-item.sidebar-link-with-icon` at the start in both states, from
  1024 px only so the phone drawer stays as Kiso draws it, with
  `padding-inline-start` of `--app-shell-brand-inset + (--size-control-sm -
  --size-icon-md) / 2 - --spacing-sm`, so the icon's center is the cat's
  center. The second selector, with `[data-collapsed="true"]`, beats Kiso's
  collapsed rule. The recipe's 7px is the link inset minus 1px for the rail
  border. Here the link inset is `--spacing-sm` and the brand inset already
  removes the 1px, so the offset is 8px. The product has no CSS that keeps
  `.sidebar-expanded-content` visible in the rail or toggles on hover.
- The sidebar is `collapsible`, with the toggle in the header. The collapse
  button is new, and the rail keeps the symbol.
- The shell header has the breadcrumb of the recipe (`HeaderBreadcrumb`),
  after the sidebar toggle: the page's host from `location.host`, with the
  port when it is not the default one for the scheme (mono, small,
  truncating, links to the app root), the list page, and on a host, role or sync run the
  record's name as `BreadcrumbPage`. It never wraps (`flex-wrap: nowrap`, the first and last item
  can shrink, the last one ellipsizes). Below 720 px only the current page
  shows, and the address stays next to the symbol in the phone header. There
  the page name has a cap (the smaller of 7rem and 16vw, then it
  ellipsizes) and never shrinks below it, and the address shrinks first,
  down to the symbol. The "Theme" label is hidden there to leave room for the address. From
  720 px the address is in the breadcrumb only. At 1024 px and above the
  sidebar bar has the lockup alone. The host and role pages no longer render
  their own breadcrumb. The sync run page still does, and `product.css`
  hides any breadcrumb inside `.page`, so the header is the only path.
- sewa has no login page, so the login lockup rule does not apply. The
  favicon is the cat, as before. The recipe's PNG app icons are not added.
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
  720 px, and a role with many items needs the room for its chips, and the
  built-in role for its Name, Manager, Version and OS columns.
  `product.css` lifts the limit for `.role-form` and for the new sync
  form (`.wide-form`), whose host picker has a filter column and a table.
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
