# Kiso in sewa

The web app follows Kiso. This file records only what Kiso leaves to the
product. The rules live in the installed contracts. If this file and a
contract disagree, the contract wins and this file has a bug.

## Contracts

- Package: `@momoi-labs/kiso-react` 0.31.0 and `@momoi-labs/kiso` 1.9.0 in
  `web/`.
- Contracts: `web/node_modules/@momoi-labs/kiso/kiso/`. Start at `AGENTS.md`.

## Paths

| What | Where |
| --- | --- |
| Screens | `web/src/hosts/`, `web/src/desired/`, `web/src/sync/`, `web/src/passkeys/` |
| Product components | `web/src/components/`, `web/src/hosts/`, `web/src/desired/`, `web/src/sync/`, `web/src/passkeys/` |
| Product CSS | `web/src/product.css`: only what Kiso leaves to the product: a LogView height, the tinted brand slot, the numbered form steps, the host picker rows, and the chip remove button exception below. `web/src/components/SewaMark.css` holds the logo colors. `web/src/components/ErrorPage.css` sizes the error illustration. |

## Product components

| Component | Use |
| --- | --- |
| `Shell` | The application shell with the Hosts, Roles, Sync and Passkeys destinations and the theme selector. Its brand slot is tinted with the accent surface and shows the bold name over the instance address from `GET /api/instance`. |
| `SewaMark` | The sewa emblem in the shell's brand. The brand kit is in `docs/brand/`. |
| `HostStatusBadge` | A host's status: waiting, connected, or disconnected. |
| `ErrorAlert` | An API failure, with what happened and what to do. |
| `ErrorPage` | The screen when a page's data fails to load with `not_admin`, a 5xx, or no answer: a brand illustration, the code, a title, one line, and one action. It fills the page with no shell, and the root route renders it. |
| `Unknown` | A missing value: a muted dash with a tooltip that says why. |
| `SkeletonRows` | Table rows while a list loads. |
| `NotFound` | The screen for an unknown path. |
| `AddHostDialog` | Creates a host and shows its install command once. |
| `AddRoleDialog` | Creates an empty role, then opens it. |
| `ItemChips` | A role's packages or mise tools as chips in a ChipInput, each with its version and OS, with suggestions from the other roles. |
| `SaveRoleDialog` | The items a role save adds, changes, and removes, and the button that saves them. |
| `ItemForm` | Adds a host adjustment: a package or a mise tool with a version. |
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

## Pattern choices

| Screen | Pattern |
| --- | --- |
| Hosts (`/`) | List-detail, separate screens: the list screen. |
| Add a host | CRUD: a small record created in a dialog. |
| Host (`/hosts/$hostId`) | List-detail, separate screens: the detail screen. |
| Roles (`/roles`) | List-detail, separate screens: the list screen. |
| Add a role | CRUD: a small record created in a dialog. |
| Role (`/roles/$roleId`) | Configuration form in numbered steps (`.form-steps`): Role, Packages, mise tools. A step with complete fields shows a check. FormActions is sticky and shows unsaved edits. Hosts get roles on the host page, not here. |
| Save a role | Confirmations: a dialog with the item diff (added, changed, removed) and "Save". |
| Sync (`/sync`) | A StepList of the run to come: the edits since the last sync, then the host picker (All, All Linux, All macOS, None), then Plans, Sign and Apply as pending. Recent syncs sit in a table below. |
| Sync run (`/sync/$syncId`) | A StepList of the run: Pick hosts, Plans, Sign, Apply. A finished step keeps a one-line summary. The hosts' apply steps are a second StepList in their own card, because a StepList nested in a step takes that step's state for every mark. A finished run shows a success or error Alert. |
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
- A chip in a ChipInput keeps its remove button in view. Kiso 1.9 lets a
  chip wider than its field scroll sideways, so on a phone a long package
  chip such as `brew-cask:ghostty` with an `os` option hid its X past the
  field's edge. `product.css` makes the remove button sticky at the chip's
  end edge, on the chip's own fill. Kiso should keep a chip's remove button
  visible.

## Verify

1. In `web/`, run `npm ci`.
2. Run `npm run lint`, `npm test`, and `npm run build`. The build runs the
   typecheck first.
3. Build and run the controller (see [api.md](api.md#web-app)), then open
   `/` on its address in both themes and at 390px wide.
