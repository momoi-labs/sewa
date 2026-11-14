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
| Screens | `web/src/hosts/` |
| Product components | `web/src/components/`, `web/src/hosts/` |
| Product CSS | None. |

## Product components

| Component | Use |
| --- | --- |
| `Shell` | The application shell with the Hosts destination and the theme selector. |
| `HostStatusBadge` | A host's status: waiting, connected, or disconnected. |
| `ErrorAlert` | An API failure, with what happened and what to do. |
| `Unknown` | A missing value: a muted dash with a tooltip that says why. |
| `SkeletonRows` | Table rows while a list loads. |
| `NotFound` | The screen for an unknown path. |
| `AddHostDialog` | Creates a host and shows its install command once. |

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

## Exceptions

None.

## Verify

1. In `web/`, run `npm ci`.
2. Run `npm run lint`, `npm test`, and `npm run build`. The build runs the
   typecheck first.
3. Build and run the controller (see [api.md](api.md#web-app)), then open
   `/` on its address in both themes and at 390px wide.
