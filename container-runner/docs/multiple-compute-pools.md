# Running two images at once on Rivet Compute

Rivet Compute runs your code in **compute pools**. Each pool is deployed with its
own image, and the runners in a pool register under the pool's name. To run two
images at the same time you deploy two pools, then use **`runner_name_selector`**
in the actor API to send each actor to the pool (image) you want.

This is the pattern behind blue/green and canary rollouts, or running a stable
and an experimental build together and choosing between them per actor.

## How it fits together

- A **compute pool** is one deployed image plus the runners that serve it. Every
  runner in a pool registers under the pool's **name**.
- You deploy a pool with `rivet deploy --pool <name>`. The first deploy to a new
  name creates the pool.
- When you create an actor you set **`runner_name_selector`** to a pool name. The
  engine schedules that actor onto a runner in that pool, so it runs that pool's
  image.

```
pool "version_a"  (image A)  <—— runner_name_selector: "version_a"
pool "version_b"  (image B)  <—— runner_name_selector: "version_b"
```

## Prerequisites

- The `rivet` CLI.
- A Rivet Cloud API token (`cloud_api_...`) for deploying, exported as
  `RIVET_CLOUD_TOKEN` or passed with `--token`.
- A namespace runtime token (`sk_...`) for the actor API, used below as
  `RIVET_TOKEN`.
- Two builds you want to run side by side.

## Step 1 — Deploy the first pool

Deploy your first image to a pool named `version_a`:

```bash
rivet deploy --pool version_a --dockerfile Dockerfile.a
```

This builds and pushes image A and provisions the `version_a` pool. Its runners
come up registered under the name `version_a`.

## Step 2 — Deploy the second pool

Deploy your second image to a pool named `version_b`:

```bash
rivet deploy --pool version_b --dockerfile Dockerfile.b
```

Both pools are now live at the same time, each running its own image. Anything
that distinguishes the builds works here — separate Dockerfiles, build contexts,
or `--image` / `--tag`. The pools are fully independent; deploying or
redeploying one never touches the other.

## Step 3 — Confirm both pools are ready

```bash
rivet pool list
```

You should see both `version_a` and `version_b` with status `ready`.

## Step 4 — Route actors to a pool

Actor creation takes a required `runner_name_selector` — set it to the pool name.
The actor then runs on that pool's image.

Get-or-create (idempotent by `key`):

```bash
curl -X PUT "https://api.rivet.dev/actors?namespace=$NAMESPACE" \
  -H "Authorization: Bearer $RIVET_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "my-actor",
    "key": "actor-1",
    "runner_name_selector": "version_a",
    "crash_policy": "restart"
  }'
```

Send the next actor to the other image just by changing the selector:

```bash
curl -X POST "https://api.rivet.dev/actors?namespace=$NAMESPACE" \
  -H "Authorization: Bearer $RIVET_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "my-actor",
    "runner_name_selector": "version_b",
    "crash_policy": "restart"
  }'
```

Same actor type, same namespace — the selector is the only thing deciding which
image handles it.

### Request fields

| Field | Required | Notes |
| --- | --- | --- |
| `name` | yes | Actor name registered in your build. |
| `runner_name_selector` | yes | Pool name to run on (e.g. `version_a`). |
| `crash_policy` | yes | `restart`, `sleep`, or `destroy`. |
| `key` | `PUT` only | Stable identity for get-or-create. |
| `input` | no | Base64-encoded binary input passed on creation. |
| `datacenter` | no | Pin the actor to a datacenter. |

`namespace` is a required query parameter. Use `PUT /actors` for get-or-create
and `POST /actors` to always create a new actor.

## What the selector does

- The engine schedules the actor onto a runner whose name equals
  `runner_name_selector`, spreading actors across the runners in that pool.
- If that pool has no runner connected or no free capacity, the actor **queues**
  until one is available. It does not spill over to the other pool.
- There is at most one live instance per actor id (single-writer). Multiple
  runners scale a pool; they never split a single actor.

## Pools vs. runner versions

These are different axes — don't confuse them:

- **Pool name (`runner_name_selector`)** picks *which image*. Use **distinct
  names** when you want two images running **at the same time** and addressable
  independently. That is what this guide does.
- **Runner version** is for **replacing** a build within one pool name: new
  version comes up, new actors go to it, old drains and migrates. Use it to roll
  a pool forward, not to run two images in parallel.

If you want both images live at once, use two pool names.

## Migration and blue/green

`runner_name_selector` is fixed for an actor once created, and existing actors
stay in their pool (rescheduling within the same name if a runner dies). So to
shift traffic:

1. Deploy `version_b` alongside `version_a` (both live).
2. Start creating new actors with `runner_name_selector: "version_b"` — send a
   fraction for a canary, or all of them for a full cutover.
3. Let the remaining `version_a` actors drain naturally, then retire the pool:

```bash
rivet pool delete version_a
```

## Using the RivetKit client

The typed RivetKit client does not expose `runner_name_selector` as a
`getOrCreate` argument; it fills it from the client's `poolName` (from
`RIVET_POOL`, default `default`). To target different pools from the client,
create one client per pool (each configured with its own `poolName` /
`RIVET_POOL`), or call the actor API directly as shown above.

## Troubleshooting

- **Actor never starts / stuck queued:** confirm a pool with exactly that
  `runner_name_selector` exists and is `ready` (`rivet pool list`). A misspelled
  pool name queues forever with no matching runner.
- **Actor ran the wrong image:** check the `runner_name_selector` you sent
  against the pool names from `rivet pool list`; they must match exactly.
