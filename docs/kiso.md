# Kiso in sewa

The web app follows Kiso. This file records only what Kiso leaves to the
product. The rules live in the installed contracts. If this file and a
contract disagree, the contract wins and this file has a bug.

## Contracts

- Package: `@momoi-labs/kiso-react` 0.30.1 and `@momoi-labs/kiso` 1.8.1 in
  `web/`.
- Contracts: `web/node_modules/@momoi-labs/kiso/kiso/`. Start at `AGENTS.md`.

## Paths

| What | Where |
| --- | --- |
| Screens | `web/src/hosts/`, `web/src/desired/`, `web/src/passkeys/` |
| Product components | `web/src/components/`, `web/src/hosts/`, `web/src/desired/`, `web/src/passkeys/` |
| Product CSS | `web/src/product.css`: only sizes Kiso leaves to the product, such as a LogView height. `web/src/components/SewaMark.css` holds the logo colors. `web/src/components/ErrorPage.css` sizes the error illustration. |

## Product components

| Component | Use |
| --- | --- |
| `Shell` | The application shell with the Hosts, Roles, Changes and Passkeys destinations and the theme selector. |
| `SewaMark` | The sewa emblem in the shell's brand. The brand kit is in `docs/brand/`. |
| `HostStatusBadge` | A host's status: waiting, connected, or disconnected. |
| `ErrorAlert` | An API failure, with what happened and what to do. |
| `ErrorPage` | The screen when a page's data fails to load with `not_admin`, a 5xx, or no answer: a brand illustration, the code, a title, one line, and one action. |
| `Unknown` | A missing value: a muted dash with a tooltip that says why. |
| `SkeletonRows` | Table rows while a list loads. |
| `NotFound` | The screen for an unknown path. |
| `AddHostDialog` | Creates a host and shows its install command once. |
| `AddRoleDialog` | Creates an empty role, then opens it. |
| `DraftNotice` | Says that edits wait in the draft change, with a link to it. |
| `DifferenceBadge` | Added, removed, or changed in the draft. |
| `ItemForm` | Adds a package or a mise tool with a version. |
| `ItemsTable` | Items with the approved version beside the draft's. |
| `PlanView` | A mise bootstrap plan as a table, removals first and in danger. |
| `ChangeStatusBadge` | A change's status: draft, approved, or discarded. |
| `ApplyResultBadge` | What a host did with an approval: waiting, applied, failed, or needs re-approval. |
| `PasskeyError` | A failed passkey prompt, or an API failure through `ErrorAlert`. |
| `AddPasskeyDialog` | Names a passkey, then lets the browser create it. |
| `ApproveDialog` | Shows the plans an approval signs, then signs them with a passkey. |
| `AdoptDialog` | Adds an undeclared package to a role or to the host, in the draft. |

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
| Role (`/roles/$roleId`) | List-detail, separate screens: the detail screen. |
| Changes (`/changes`) | List-detail, separate screens: the list screen. |
| Change (`/changes/$changeId`) | List-detail, separate screens: the detail screen. Host plans sit side by side in a PaneGrid. |
| Discard a draft | Destructive actions: a confirmation dialog. |
| Passkeys (`/passkeys`) | CRUD: a list, with each passkey created in a dialog. |
| Approve a change | Confirmations: a dialog that states what is signed and applied, and names the action. |
| Adopt a package | CRUD: a small edit made in a dialog. |
| Revert and Ignore drift | Plain actions on the host's Drift tab. Revert opens a change, which still needs an approval; Ignore only hides. |
| Error page | Permission denied (403 `not_admin`): a locked EmptyState that replaces the page, with "Check again". Errors (5xx, no answer): the same composition with "Retry". Other load errors, and a failed refresh of data already on screen, keep `ErrorAlert`. |

## Exceptions

- The shell's brand shows the sewa emblem with `SewaMark` in place of a
  BrandMark, 32 px high, with no fill behind it. Kiso 1.8's BrandMark holds a
  letter or a 14 px line icon on the primary fill, and the emblem cannot be
  read at that size. Its colors are the brand's, in `SewaMark.css`, not Kiso
  tokens, so it keeps its colors with any accent. Kiso needs a BrandMark for
  a product logo.
- `ErrorPage` puts a brand illustration, about 192 px high, in an
  EmptyState, and uses that composition for 5xx errors too. Kiso prefers a
  small icon in EmptyState and an Alert for errors. The brand asks for the
  illustrated page when the whole screen cannot load. The page keeps Kiso's
  copy order (what, why, now), uses no danger color, and the 5xx page is an
  `alert` region. The illustrations are the brand's files in
  `web/public/`, not Kiso tokens. Kiso has no illustration size token, so the
  height is `--spacing-4xl` times 3.

## Verify

1. In `web/`, run `npm ci`.
2. Run `npm run lint`, `npm test`, and `npm run build`. The build runs the
   typecheck first.
3. Build and run the controller (see [api.md](api.md#web-app)), then open
   `/` on its address in both themes and at 390px wide.
