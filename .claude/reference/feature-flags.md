# Frontend Feature Flags

The dashboard runs against several deployment flavors (full cloud, OSS self-host, enterprise/on-prem). A feature flag lets each flavor turn a capability on or off without forking the code. Flags are the mechanism that keeps one frontend build serving every flavor.

## Where flags live

All flags are defined in one place: [frontend/src/lib/features.ts](../../frontend/src/lib/features.ts). Consume them with:

```ts
import { features } from "@/lib/features";

if (features.platform) {
  // cloud-platform-only UI
}
```

- Source of truth at runtime is the `VITE_FEATURE_FLAGS` env var: a comma-separated list of enabled flag names.
- In dev (`import.meta.env.DEV`), a `localStorage` key `FEATURE_FLAGS` overrides the env var so a flavor can be simulated locally.
- **Unset (`undefined`) means every flag is on** — that is the full cloud build. An empty/explicit list opts in only to the named flags.
- Some flags imply others (e.g. `platform` requires `auth`; `acl` is implied by `platform`). Encode those dependencies in `features.ts`, not at each call site.

## Current flags

| Flag | Meaning |
| --- | --- |
| `auth` | Dashboard ships a login/register flow. NOT a proxy for "engine requires credentials." |
| `platform` | Cloud platform stack: publishable-token endpoint, billing, projects, multi-tenancy. Implies `auth`. (`multitenancy` is a legacy alias accepted during rollover.) |
| `acl` | Engine enforces token auth on the public endpoint. Implied by `platform`; set independently for enterprise. |
| `billing` | Billing UI. |
| `captcha` | Turnstile captcha on auth forms. Requires `auth`. |
| `compute` | Rivet Compute (managed pool) UI: namespace deployments + logs sidebar links and routes, actor-details deployment-logs tab, and the Rivet provider option in onboarding / Add Provider. Requires `platform`. |
| `support` | Support/help affordances. |
| `branding` | Rivet branding chrome. |
| `datacenter` | Datacenter-related UI. |
| `danger-zone` | Destructive settings actions (`features.dangerZone`). |
| `mcp` | MCP connection settings card on the namespace settings drawer. Flavor-dependent content: `platform` renders the hosted `mcp.rivet.dev` endpoint pinned to this namespace, everything else renders the local stdio (`npx @rivet-dev/mcp`) config. |

Deployment flavors map to flag sets roughly as: **cloud** = all on; **OSS** = `auth`/`platform`/`acl` off; **enterprise** = `acl` on, `auth`/`platform` off (engine enforces auth without a login UI). Do not treat `platform`/`auth` as "engine requires credentials" — that is `acl`. **`compute` is opt-in even on cloud** — each Railway service adds it to `VITE_FEATURE_FLAGS` per-environment (e.g. staging on, prod off) rather than inheriting the cloud default-on set.

## Testing across flavors (required for frontend changes)

A bug can exist in only one flavor. Whatever flavor your dev server happens to default to hides breakage in the others, so testing a single flavor is not enough. **OSS is the most important and the most commonly regressed flavor** because it turns the most off (`auth`/`platform`/`acl` all off), so any code that assumes cloud context (org/project params, auth session, cloud data providers) silently breaks there. The OSS namespace dropdown, sidebar, context switcher, and onboarding all take different code paths than cloud.

**Before considering any frontend change done, verify it in at least OSS and cloud.** If a surface renders conditionally on `features.*` (or differs by flavor at all: sidebar, context switcher, onboarding, settings, auth, billing), exercise each affected flavor in the browser, not just the default.

Switch flavors in dev without restarting the server by setting the `localStorage` override and reloading (the dev build reads `localStorage.FEATURE_FLAGS` ahead of `VITE_FEATURE_FLAGS`):

```js
// OSS self-host: everything off
localStorage.setItem("FEATURE_FLAGS", ""); location.reload();

// OSS self-host with the local MCP snippet shown
localStorage.setItem("FEATURE_FLAGS", "mcp"); location.reload();

// Full cloud: all flags on (see the commented canonical list in frontend/.env.local)
localStorage.setItem(
  "FEATURE_FLAGS",
  "compute,platform,acl,auth,captcha,branding,support,billing,datacenter,danger-zone,multitenancy,mcp",
);
location.reload();

// Enterprise: acl on, no login UI
localStorage.setItem("FEATURE_FLAGS", "acl,branding,support,datacenter,danger-zone,mcp"); location.reload();
```

In an agent-browser / DevTools session, paste those into the page console. `localStorage` persists across reloads, so run `localStorage.removeItem("FEATURE_FLAGS")` to return to the env default when finished. Confirm the active flavor with `JSON.stringify(features)` after importing, or just observe whether auth/cloud chrome is present.

## When to add a new flag

**A new pack of features must ship behind a feature flag whenever it is significant or not universally available across deployment flavors.** The goal is that every flavor can freely enable or disable it.

Add a flag when the feature:

- Is unavailable, restricted, or behaves differently on at least one deployment flavor (cloud / OSS / enterprise), OR
- Introduces a substantial new surface (a whole panel, page, settings section, or subsystem) that a flavor may want to turn off.

Do **not** add a flag for:

- Small, universal changes (bug fixes, copy tweaks, layout polish, a single button that every flavor always shows).
- Anything that is always on everywhere — that is just code.

Avoid flag sprawl: do not gate everything. One flag per meaningful capability, not per component.

**If you are unsure whether a feature needs a flag, confirm with the user before adding (or omitting) one.** When in doubt, ask rather than guessing — flags are hard to remove once flavors depend on them.

## Consistency rules

- Add the flag in `features.ts` with a one-line comment on what it gates and any implied dependencies; do not read `VITE_FEATURE_FLAGS` or `import.meta.env` directly elsewhere.
- Name flags after the capability (`billing`, `support`), not the deployment (`enterprise-only`). Deployment flavor is composed from flags, not the other way around.
- Encode flag-implies-flag relationships in `features.ts` so call sites stay simple booleans.
- When a flag changes the set of flavors, update the deployment-flavor mapping above and any docs that describe what each flavor ships.
