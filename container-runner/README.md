# Container Runner

User-facing documentation lives at
[rivet.dev/docs/deploy/container-runner](https://rivet.dev/docs/deploy/container-runner/).
This README covers development, the example projects, and the local test harnesses.

`rivet-container-runner` is a RivetKit (Rust) serverless app that hosts actors by
spawning a child game-server process per actor and proxying Rivet's tunneled
HTTP/WebSocket traffic to it. Each child gets its own port; the pool's request
concurrency decides how many actors share a container (one, in the recommended
game-server setup). The instance stays warm after its last actor stops; the engine
reaps it by draining the `/start` connection after the request lifespan. Wrap any dedicated server (Unity, Godot, a plain Node process) in a
container with this binary as the entrypoint and Rivet Compute can cold-start and route
to it.

The actor `input` payload (CBOR-encoded, RivetKit convention) can override the child
command, args, and env per actor. WebSocket clients connect at the bare gateway
path; raw HTTP reaches the child under the `/request/*` prefix on the actor surface.

`examples/` contains a Unity + FishNet demo project, a lightweight Node test server, and
an end-to-end test harness. All commands below run from the rivet repo root.

There are two working local paths:

1. Run the FishNet project locally in Unity.
2. Run the built Unity server behind `container-runner`, create a Rivet actor locally,
   and connect a FishNet client through the local Rivet guard URL.

A production image for **Rivet Compute** (the Cloud Run serverless model) is provided —
see [Rivet Compute](#rivet-compute) below.

## Project Layout

| Path | What |
|------|------|
| `examples/unity-demo/` | Standard Unity project with a simple FishNet + Bayou scene |
| `src/` | Rust runner that spawns a child game server and proxies Rivet traffic to it |
| `examples/e2e-test/` | Local Rivet engine test harness |
| `examples/test-server/` | Small Node WebSocket server used as a fast runner/tunnel regression test |

The main Unity scene is:

```text
examples/unity-demo/Assets/Scenes/Main.unity
```

## Prerequisites

- Unity `6000.5.2f1`
- An activated Unity license in Unity Hub
- Rust/Cargo
- Node.js

The scripts assume this Unity editor path unless `UNITY` is overridden:

```bash
/Applications/Unity/Hub/Editor/6000.5.2f1/Unity.app/Contents/MacOS/Unity
```

## Local Unity Dev

Open `examples/unity-demo/` in Unity.

Use this scene:

```text
Assets/Scenes/Main.unity
```

If the scene needs to be regenerated or rewired, run:

```text
Tools -> Setup FishNet Multiplayer Demo
```

The scene contains:

- `NetworkManager`
- FishNet managers
- Bayou WebSocket transport
- `ServerBootstrap`
- a simple floor
- a networked player prefab
- spawn points

In the Unity editor, pressing Play starts the server and a local client automatically.
Move the local player with WASD or arrow keys.

## Build The Unity Demo

From the rivet repo root:

```bash
UNITY=/Applications/Unity/Hub/Editor/6000.5.2f1/Unity.app/Contents/MacOS/Unity \
  container-runner/examples/unity-demo/setup-and-build.sh demo
```

This creates a macOS player under:

```text
container-runner/examples/unity-demo/Builds/DemoMac/
```

The demo player can run as either server or client. The e2e harness uses one copy as
the server child and another copy as the client.

## Local Dev With Container Runner

First run the fast Node child test:

```bash
container-runner/examples/e2e-test/host/run-host.sh
```

Expected result:

```text
PASS: full host e2e (engine -> container-runner -> child) succeeded.
```

Then run the Unity/FishNet path:

```bash
container-runner/examples/e2e-test/host/run-host-fishnet.sh
```

Expected result:

```text
PASS  FishNet client connected through Rivet
PASS  Unity server accepted FishNet client
```

That script:

1. Starts the local Rivet engine.
2. Starts `container-runner`.
3. Creates a local Rivet actor.
4. Spawns the Unity server as the runner's child process.
5. Launches a Unity FishNet client.
6. Connects the client through the local Rivet guard:

```text
ws://127.0.0.1:7420/gateway/<actor_id>/
```

The WebSocket subprotocol is:

```text
rivet
```

Useful logs:

```bash
tail -100 container-runner/examples/e2e-test/.host-logs/engine.log
tail -100 container-runner/examples/e2e-test/.host-logs/runner-fishnet.log
tail -100 container-runner/examples/e2e-test/.host-logs/client-fishnet.log
cat container-runner/examples/e2e-test/.host-logs/create-fishnet.json
```

## Manual Actor URL Flow

The intended client pattern is:

```bash
node container-runner/examples/e2e-test/create-actor.mjs --json
```

The script creates an actor and returns the public URL clients should use:

```json
{
  "ws_url": "ws://127.0.0.1:7420/gateway/<actor_id>/",
  "websocket_subprotocol": "rivet"
}
```

The FishNet client must connect to that `ws_url` through Rivet, not directly to the
Unity server port.

## Load Test

`examples/e2e-test/load-test.mjs` spawns `N` actors and drives a WebSocket **ping-pong** round-trip
through the Rivet guard to each one, reporting create/round-trip success rates and latency
percentiles. The workload child is the lightweight `test-server` (echoes `echo: <msg>`),
**not** a Unity server — so thousands can run cheaply; this stresses the Rivet serverless +
`container-runner` path, not the game.

Run it locally with the host harness, which stands up one `container-runner` instance
(= one container) per actor, each in its own serverless pool:

```bash
LOAD_COUNT=50 container-runner/examples/e2e-test/host/run-host-loadtest.sh
```

Expected tail:

```text
[create] 50/50 ok ...
[pingpong] 50/50 ok ...
[pingpong] round-trip ms: p50=... p95=... p99=... max=...
PASS: host container load test (50 containers) succeeded.
```

Knobs: `LOAD_COUNT` (default 25), `LOAD_CONCURRENCY` (default 64).

**Running the full 1000:** 1000 local instances is ~2000 processes (a Rust runner + Node
child each) and needs a beefy host plus a raised `ulimit -n`. For a true 1000-container run,
point `load-test.mjs` at **Rivet Cloud** instead — Cloud Run scales the containers, no local
limit. Set the engine env and let a single pool auto-scale:

```bash
ENGINE_URL=https://api.<region>.rivet.dev ENGINE_PUBLIC_URL=https://api.<region>.rivet.dev \
RIVET_NAMESPACE=<namespace> RIVET_TOKEN=<engine-token> LOAD_GATEWAY_TOKEN=<token> \
RIVET_RUNNER_NAME=default LOAD_DATACENTER=eu-central-1 LOAD_COUNT=1000 \
  node container-runner/examples/e2e-test/load-test.mjs
```

`LOAD_GATEWAY_TOKEN` is embedded in the gateway path as `<actor_id>@<token>` (how Rivet
Cloud authenticates header-less WebSocket clients).

## Docker Note

The Docker e2e path is useful on Linux:

```bash
cd container-runner/examples/e2e-test
./run.sh
```

On Docker Desktop for macOS, the full actor spawn path is known to fail because the
local Rivet engine cannot reliably reach sibling containers through Docker's network.
Use the host runners above on macOS.

## Rivet Compute

Rivet Compute runs your container on a serverless model: the engine
cold-starts the container and drives it over the serverless protocol
(`POST /api/rivet/start`) on the port it injects as `$RIVET_PORT`. That is exactly the
front door `container-runner` already implements locally, so the same image works
unchanged — only the port and engine endpoint differ.

The production image is `container-runner/examples/unity-demo/Dockerfile`. It bundles the Unity Linux
dedicated server, wrapped by `rivet-container-runner`:

```text
Rivet Compute (public HTTPS)  ->  rivet-container-runner  ->  Unity Linux server
        ^  POST /api/rivet/start on $RIVET_PORT
   Rivet engine
```

### 1. Install Unity Linux Build Support

The image needs a **Linux** dedicated-server build (the local build is macOS). In Unity
Hub: Installs -> `6000.5.2f1` -> gear -> Add Modules -> **Linux Build Support
(IL2CPP/Mono)** and **Linux Dedicated Server**.

### 2. Build the Linux server

```bash
container-runner/examples/unity-demo/setup-and-build.sh linux
# -> container-runner/examples/unity-demo/Builds/ServerLinux/GameServer  (+ GameServer_Data/, UnityPlayer.so)
```

### 3. (Optional) smoke-test the image locally

The deploy in step 4 builds the image for you, so this is only to sanity-check the front
door before shipping:

```bash
docker build --platform linux/amd64 -f container-runner/examples/unity-demo/Dockerfile -t game-unity:amd64 .
docker run --rm -p 8080:8080 game-unity:amd64
curl -s localhost:8080/api/rivet/health   # -> ok
```

### 4. Deploy

The Rivet CLI builds the image from the Dockerfile, pushes it to Rivet's registry, and
rolls it out to your Rivet Compute pool — one command:

```bash
export RIVET_CLOUD_TOKEN=...   # Rivet dashboard -> Connect -> Rivet Cloud
npx @rivetkit/cli deploy \
  --token "$RIVET_CLOUD_TOKEN" \
  --instance-request-concurrency 1 \
  --dockerfile container-runner/examples/unity-demo/Dockerfile
```

It ends on `pool status status=ready` and prints a dashboard link. Docker must be running;
the CLI runs `docker build` itself, so there's no manual build/push/tag step.

Notes:

- Add `--namespace <name>` if you are not deploying to the CLI's default (`production`).
- Swap in `--dockerfile container-runner/examples/test-server/Dockerfile.pingpong` to deploy the lightweight
  ping-pong image instead of Unity (used by the [load test](#load-test)).

### 5. Connect clients

Copy the connection URL from the Rivet dashboard (**Connect**). It embeds the namespace
and a publishable token as userinfo:

```text
https://<namespace>:<pk_token>@api.rivet.dev
```

That is the only input the client script needs:

```bash
RIVET_URL='https://<namespace>:<pk_token>@api.rivet.dev' \
  ./container-runner/examples/e2e-test/run-cloud-clients.sh 3
```

This creates one actor (reusing it on re-run) and opens three windowed FishNet clients
against it:

```text
==> creating actor on Rivet Compute
    actor=1cdy1y81dg0vvjxoz6hf2sggrql610
==> launching 3 client(s)
    PASS  client 1 connected and holding
    PASS  client 2 connected and holding
    PASS  client 3 connected and holding
```

Confirm from the server side:

```bash
npx @rivetkit/cli logs --token "$RIVET_CLOUD_TOKEN" --namespace <namespace> \
  | grep 'SERVER ACCEPTED CLIENT'
```

The first connect cold-starts the container, so the Unity server takes a few seconds to
boot on the initial run.

#### How the client URL is built

`examples/e2e-test/create-cloud-actor.mjs` splits `RIVET_URL` into its parts and reassembles them
into a gateway URL, because the two carry auth differently:

| | Auth | Used by |
|---|---|---|
| `RIVET_URL` | userinfo (`<namespace>:<token>@host`) | the actor REST API, as `Bearer <token>` |
| gateway URL | path (`/gateway/<actor_id>@<token>/`) | the game client |

The client can't send an `Authorization` header — Bayou and browser WebSockets have no
way to set one — so the token moves into the path. The WebSocket subprotocol is `rivet`.

Two values that are easy to get wrong:

- `runner_name_selector` must be **`default`** (the pool the CLI creates), not the actor
  name `game`. Using `game` fails with `no_runner_config_configured`.
- The engine API namespace is the **engine** namespace (e.g. `myproj-abc1-production-x2y3`),
  which differs from the CLI's `--namespace` (e.g. `production-x2y3`). `RIVET_URL` already
  carries the right one.
