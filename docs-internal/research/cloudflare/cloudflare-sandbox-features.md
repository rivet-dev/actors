# Cloudflare Sandbox SDK -- Complete Feature Reference

> **As of:** 2026-03-06
>
> **Cloudflare basis:** Official Sandbox docs accessed 2026-03-06. Cloudflare labels Sandbox SDK as beta in the docs and documents the package as `@cloudflare/sandbox`.
>
> **Rivet basis:** RivetKit 2.1.5, repo `ba46891b1`, canonical docs under `https://rivet.dev/docs/...`.
>
> **Migration framing:** Rivet Actors are **not** a container or process sandbox runtime. The best-fit migration is to keep sandbox/container execution on an external system and use Rivet Actors as the durable orchestration, routing, state, auth, and realtime control plane.
>
> **Status legend:** `native` = first-class Rivet feature, `partial` = supported with material semantic gaps, `pattern` = implemented as an application pattern on top of Rivet, `external` = requires a non-Rivet dependency/service, `unsupported` = no acceptable Rivet equivalent today, `out-of-scope` = operational/platform concern outside the Rivet Actor runtime.

## Migration Matrix

| Feature | Description | Status | Confidence | Rivet source | Validation proof | Risk | Notes |
|---------|-------------|--------|------------|--------------|------------------|------|-------|
| Sandbox Creation and Identity (`getSandbox`) | Create or retrieve sandboxes by unique string ID with lazy container start | partial | high | [Actor Keys](https://rivet.dev/docs/actors/keys), [Lifecycle](https://rivet.dev/docs/actors/lifecycle) | [actor-handle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-handle.ts) | Medium | Rivet can provide deterministic controller identity, but not native container provisioning. |
| Sandbox Lifecycle Management | Four lifecycle states: creation, active, idle/sleep, and destruction | partial | high | [Lifecycle](https://rivet.dev/docs/actors/lifecycle), [Destroy](https://rivet.dev/docs/actors/destroy) | [actor-lifecycle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-lifecycle.ts), [actor-destroy.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-destroy.ts) | High | Lifecycle works for the orchestration actor, not for a native sandbox container. |
| Ephemeral State Model | State persists only during active container periods, resets on sleep | pattern | high | [State](https://rivet.dev/docs/actors/state), [Ephemeral Variables](https://rivet.dev/docs/actors/ephemeral-variables) | [actor-vars.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-vars.ts), [actor-sleep.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-sleep.ts) | Medium | Model durable control state in `state` and transient process handles in `vars`. |
| Three-Layer Architecture | Client SDK, Durable Object, and Container Runtime layered design | out-of-scope | high | [Actors Index](https://rivet.dev/docs/actors) | Docs-only | Medium | Rivet covers the orchestration layer, not the full container-runtime stack. |
| Command Execution (`exec`) | Execute shell commands capturing stdout, stderr, and exit codes | external | high | [Workflows](https://rivet.dev/docs/actors/workflows), [AI and User-Generated Rivet Actors](https://rivet.dev/docs/actors/ai-and-user-generated-actors) | Gap | High | Requires an external sandbox/worker/container service. |
| Streaming Command Execution (`execStream`) | Real-time SSE streams for command stdout, stderr, and completion events | external | high | [Realtime](https://rivet.dev/docs/actors/events), [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | Gap | High | You can proxy stream output through Rivet after an external runtime emits it. |
| Background Processes (`startProcess`) | Start and manage long-running processes like servers and services | external | high | [Workflows](https://rivet.dev/docs/actors/workflows) | Gap | High | Rivet can supervise metadata and routing, not run the process itself. |
| File System Access | Read, write, delete, rename, and manage files and directories | external | high | [Low-Level KV Storage](https://rivet.dev/docs/actors/kv) | Gap | High | `c.kv` is not a mounted POSIX filesystem. |
| File Watching | Real-time filesystem monitoring using Linux inotify with filtering | unsupported | high | [Actors Index](https://rivet.dev/docs/actors) | Gap | High | No filesystem watcher surface exists in Rivet Actors. |
| Code Interpreter | Execute Python, JavaScript, and TypeScript with persistent state contexts | external | high | [Workflows](https://rivet.dev/docs/actors/workflows) | Gap | High | Use an external interpreter runtime and persist orchestration state in Rivet. |
| Sessions (Shell Execution Contexts) | Isolated bash shell contexts with independent environment and working directory | external | high | [Connections](https://rivet.dev/docs/actors/connections), [Realtime](https://rivet.dev/docs/actors/events) | Gap | High | Model sessions as actor state plus an external runtime session ID. |
| Preview URLs (Port Exposure) | Public HTTPS access to sandbox services via exposed ports | pattern | medium | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler), [Deploying to Cloudflare Workers](https://rivet.dev/docs/connect/cloudflare-workers) | [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts) | High | Rivet can proxy or route to sandbox services, but has no native per-port preview URL primitive. |
| Terminal Connections (Interactive Shells) | WebSocket-based interactive terminal sessions with xterm.js integration | pattern | medium | [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts) | High | Rivet can proxy a terminal stream over WebSockets, but does not provide shell emulation itself. |
| Backup and Restore | Point-in-time squashfs snapshots stored in R2 with FUSE overlay restore | external | high | [Workflows](https://rivet.dev/docs/actors/workflows) | Gap | High | Requires an external snapshot/storage system. |
| Storage (S3-Compatible Bucket Mounting) | Mount R2, S3, or GCS buckets as local filesystems via `s3fs-fuse` | external | high | [Low-Level KV Storage](https://rivet.dev/docs/actors/kv) | Gap | High | Rivet offers KV and SQLite, not filesystem mounts. |
| Transport Modes | HTTP or WebSocket communication between SDK and container | native | high | [Low-Level HTTP Request Handler](https://rivet.dev/docs/actors/request-handler), [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts), [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts) | Low | Good fit for the orchestration/control plane. |
| Dockerfile Configuration (Container Images) | Base image variants and custom Dockerfile support for container setup | external | high | [AI and User-Generated Rivet Actors](https://rivet.dev/docs/actors/ai-and-user-generated-actors) | Gap | High | Keep image build/runtime configuration in the external sandbox provider. |
| Wrangler Configuration | Minimal wrangler config for containers, Durable Objects, and migrations | pattern | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers/wrangler.json](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/wrangler.json) | Low | Relevant only if you host the Rivet entrypoint on Cloudflare Workers. |
| Environment Variables | SDK config vars and three methods for setting container env vars | partial | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers), [Authentication](https://rivet.dev/docs/actors/authentication) | [examples/cloudflare-workers/src/index.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/src/index.ts) | Medium | Rivet supports normal app env vars, but not native per-process env injection for a sandbox runtime. |
| Sandbox Options (Container Timeouts) | Configurable provisioning and port readiness timeouts with ID normalization | partial | medium | [Lifecycle](https://rivet.dev/docs/actors/lifecycle), [Limits](https://rivet.dev/docs/actors/limits) | Docs-only | Medium | Rivet has action/lifecycle timeouts, not container boot/readiness controls. |
| Security Model | VM-level isolation between sandboxes with developer auth responsibilities | partial | medium | [Authentication](https://rivet.dev/docs/actors/authentication), [Connections](https://rivet.dev/docs/actors/connections) | [access-control.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/access-control.ts) | High | Rivet can enforce auth and routing, but does not provide VM-level sandbox isolation. |
| Container Runtime Details | Network capabilities, limitations, and reserved ports | unsupported | high | [Actors Index](https://rivet.dev/docs/actors) | Gap | High | No container runtime exists to compare. |
| Git Workflows | Clone repos, checkout branches, and commit changes within sandboxes | external | high | [Workflows](https://rivet.dev/docs/actors/workflows) | Gap | High | Requires external execution environment and filesystem. |
| Docker-in-Docker | Run Docker commands within sandbox containers using rootless mode | unsupported | high | [Actors Index](https://rivet.dev/docs/actors) | Gap | High | No native container execution surface. |
| WebSocket Connections to Sandbox Services | Connect to WebSocket servers via preview URLs or `wsConnect` proxy | pattern | medium | [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) | [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts) | Medium | Can be proxied through Rivet or your edge app. |
| Streaming Output | Real-time output from commands via `execStream` and `streamProcessLogs` | pattern | medium | [Realtime](https://rivet.dev/docs/actors/events), [Connections](https://rivet.dev/docs/actors/connections) | [examples/sandbox/src/actors/http/raw-websocket-chat-room.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/sandbox/src/actors/http/raw-websocket-chat-room.ts) | Medium | Works well once an external runtime emits structured output. |
| Production Deployment (Custom Domains) | Custom domain setup with wildcard DNS and TLS for exposed ports | partial | medium | [Deploying to Cloudflare Workers](https://rivet.dev/docs/connect/cloudflare-workers) | [examples/cloudflare-workers/README.md](https://github.com/rivet-dev/rivet/blob/ba46891b1/examples/cloudflare-workers/README.md) | Medium | Rivet supports normal app deployment, but not wildcard per-sandbox port routing as a primitive. |
| Logging Configuration | Configurable log level and format via environment variables | native | high | [Debugging](https://rivet.dev/docs/actors/debugging) | [actor-inspector.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-inspector.ts) | Low | Strong fit for orchestration logs. |
| Instance Types and Resource Limits | Six predefined instance types from lite to standard-4 with custom options | unsupported | high | [Limits](https://rivet.dev/docs/actors/limits) | Docs-only | High | Rivet exposes actor/runtime limits, not sandbox VM SKU selection. |
| Pricing | Per-10ms billing for memory, CPU, disk, and network egress | out-of-scope | high | [Actors Index](https://rivet.dev/docs/actors) | Docs-only | Low | Commercial comparison, not feature parity. |
| Version Compatibility | SDK npm package version must match Docker container image version | pattern | medium | [Versions](https://rivet.dev/docs/actors/versions) | Docs-only | Medium | Rivet versioning exists for actor code/runtime, but external sandbox image compatibility remains outside Rivet. |
| Request Routing and Geography | First request determines location, subsequent requests route to same region | partial | medium | [Metadata](https://rivet.dev/docs/actors/metadata), [Multi-Region](https://rivet.dev/docs/self-hosting/multi-region) | Docs-only | Medium | Region-aware routing exists, but not Cloudflare Sandbox's exact placement model. |
| Integration with Workers (Minimal Example) | Minimal Worker example executing commands and managing files | partial | medium | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/cloudflare-workers) | High | Rivet integrates with Workers, but not for native command/file execution. |
| Local Development | Docker-based local dev via wrangler dev with `EXPOSE` port declarations | partial | medium | [Testing](https://rivet.dev/docs/actors/testing), [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | [examples/cloudflare-workers](https://github.com/rivet-dev/rivet/tree/ba46891b1/examples/cloudflare-workers) | Medium | Local development exists for the orchestration layer only. |
| Wrangler CLI Commands | Project creation, local development, and deployment CLI commands | out-of-scope | high | [Cloudflare Workers Quickstart](https://rivet.dev/docs/actors/quickstart/cloudflare-workers) | Gap | Low | No equivalent sandbox control-plane CLI exists in Rivet. |

## High-Risk Behavioral Deltas

- **Rivet is the control plane, not the sandbox runtime.** If the Cloudflare design depends on native process execution, filesystem semantics, port exposure, or container images, keep that layer external.
- **Auth and realtime map well; execution does not.** Rivet is strong for identity, orchestration, workflow state, WebSocket fanout, and durable control logic around a sandbox fleet.
- **Preview URLs and terminal access are proxy patterns.** They can be built through `onRequest` and `onWebSocket`, but they are not first-class primitives.
- **Resource model and cost model do not map.** Actor limits are not VM SKU selection. Do not treat Rivet limits as a substitute for sandbox resource isolation controls.
- **Recovery and storage tooling remain an external concern.** Snapshotting, mounted buckets, Docker-in-Docker, and Git/file workflows all stay outside the Rivet Actor runtime.

## Validation Checklist

| Test case | Expected result | Pass/fail evidence link |
|-----------|-----------------|-------------------------|
| Durable controller identity exists | One actor key maps to one sandbox controller record | Pass: [actor-handle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-handle.ts) |
| Orchestration lifecycle is durable | Controller actor can sleep, wake, and destroy cleanly | Pass: [actor-lifecycle.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-lifecycle.ts), [actor-destroy.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/actor-destroy.ts) |
| HTTP/WS proxy surface works | Actor can proxy sandbox control traffic over HTTP/WebSocket | Pass: [raw-http.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-http.ts), [raw-websocket.ts](https://github.com/rivet-dev/rivet/blob/ba46891b1/rivetkit-typescript/packages/rivetkit/src/driver-test-suite/tests/raw-websocket.ts) |
| External exec runtime is chosen | Migration plan names the non-Rivet sandbox/container provider | Gap: use [AI and User-Generated Rivet Actors](https://rivet.dev/docs/actors/ai-and-user-generated-actors) only as orchestration guidance; choose an external sandbox provider separately |
| File system and snapshot requirements are replaced | External storage/runtime path is documented for files, backups, and mounts | Gap: [Low-Level KV Storage](https://rivet.dev/docs/actors/kv) is not a filesystem; choose an external storage/runtime path |
| Terminal UX is proven | Terminal protocol and browser client work end-to-end through the chosen proxy pattern | Gap: only [Low-Level WebSocket Handler](https://rivet.dev/docs/actors/websocket-handler) primitives are documented; add end-to-end proxy proof |
| Security boundary is acceptable | Team signs off that auth/routing plus external sandbox isolation meets requirements | Gap: combine [Authentication](https://rivet.dev/docs/actors/authentication) with the external sandbox provider's isolation model in a design review |

---

> **Purpose:** This document catalogs every feature of Cloudflare Sandbox (built on Cloudflare Containers) for comparison with Rivet Actors. Each feature includes a description, link to docs, and code snippets taken directly from the documentation.
>
> **Status:** Cloudflare Sandbox SDK is in **Beta**. It requires a **Workers Paid plan** ($5/month).
>
> **Package:** `@cloudflare/sandbox`

---

## Sandbox Creation and Identity (getSandbox)

**Docs:** https://developers.cloudflare.com/sandbox/api/lifecycle/

Sandboxes are created or retrieved by a unique string ID via `getSandbox()`. The container starts **lazily** on first operation -- calling `getSandbox()` returns immediately and the container only spins up when you execute a command. Each sandbox is backed by a Cloudflare Durable Object.

```typescript
import { getSandbox, type Sandbox } from "@cloudflare/sandbox";

export { Sandbox } from "@cloudflare/sandbox";

type Env = {
  Sandbox: DurableObjectNamespace<Sandbox>;
};

// Get or create a sandbox instance
const sandbox = getSandbox(env.Sandbox, "my-sandbox");
```

Naming strategies from the docs:

- **Per-user:** `user-${userId}` -- persistence during active sessions
- **Per-session:** `session-${Date.now()}-${Math.random()}` -- fresh environments
- **Per-task:** `build-${repoName}-${commit}` -- idempotent build operations

Optional configuration parameters:

```typescript
const sandbox = getSandbox(env.Sandbox, "my-sandbox", {
  sleepAfter: "10m",       // Duration before auto-sleep (default: "10m")
  keepAlive: false,        // Heartbeat pings to prevent eviction (default: false)
  containerTimeouts: {
    instanceGetTimeoutMS: 30000,  // Provisioning wait (default: 30000)
    portReadyTimeoutMS: 90000,    // API readiness wait (default: 90000)
  },
  normalizeId: false,      // Lowercase sandbox IDs for DNS compat (default: false)
});
```

---

## Sandbox Lifecycle Management

**Docs:** https://developers.cloudflare.com/sandbox/concepts/sandboxes/

Sandboxes have four lifecycle states:

1. **Creation** -- Instantiated on first reference via `getSandbox()`.
2. **Active** -- Container processes requests; all state (files, processes, shell sessions, env vars) is preserved.
3. **Idle/Sleep** -- After 10 minutes of inactivity (configurable via `sleepAfter`), containers stop. When new requests arrive, a fresh container starts with reset state.
4. **Destruction** -- Explicit via `destroy()` or automatic cleanup.

### Keep-Alive

**Docs:** https://developers.cloudflare.com/sandbox/api/lifecycle/

When `keepAlive: true` is set, the sandbox automatically sends heartbeat pings every 30 seconds to prevent container eviction. You **must** call `destroy()` when finished to prevent containers running indefinitely.

```typescript
const sandbox = getSandbox(env.Sandbox, "user-123", {
  keepAlive: true,
});

// Toggle dynamically
await sandbox.setKeepAlive(false);
```

### Sleep After (Idle Timeout)

**Docs:** https://developers.cloudflare.com/sandbox/configuration/sandbox-options/

```typescript
const sandbox = getSandbox(env.Sandbox, 'user-123', {
  sleepAfter: '30s'
});

const sandbox2 = getSandbox(env.Sandbox, 'user-456', {
  sleepAfter: 300  // 300 seconds = 5 minutes
});
```

Accepts duration strings like `"30s"`, `"5m"`, `"1h"` or numbers representing seconds.

### Destroy

**Docs:** https://developers.cloudflare.com/sandbox/api/lifecycle/

Terminates containers immediately. Destroys all files in `/workspace`, `/tmp`, `/home`, all processes, sessions, and network connections.

```typescript
await sandbox.destroy();
```

---

## Ephemeral State Model

**Docs:** https://developers.cloudflare.com/sandbox/concepts/sandboxes/

State persists **only** during active container periods. When the container sleeps or restarts:

- All files are deleted
- All processes terminate
- Shell state resets
- Code contexts clear

While running, files in `/workspace`, `/tmp`, `/home` remain accessible, background processes continue, and shell/interpreter contexts preserve their state.

---

## Three-Layer Architecture

**Docs:** https://developers.cloudflare.com/sandbox/concepts/architecture/

The Sandbox SDK combines three Cloudflare technologies:

1. **Layer 1: Client SDK** -- TypeScript interface for invoking sandbox operations
2. **Layer 2: Durable Object** -- Manages sandbox lifecycle, routes requests, manages preview URLs and state
3. **Layer 3: Container Runtime** -- Isolated Linux environment with VM-based isolation; each sandbox runs in its own VM

```typescript
// Layer 1: Client SDK calls
const sandbox = getSandbox(env.Sandbox, "my-sandbox");

// Layer 2: Durable Object routes to container
const result = await sandbox.exec("python script.py");
```

---

## Command Execution (exec)

**Docs:** https://developers.cloudflare.com/sandbox/api/commands/

Execute shell commands and capture stdout, stderr, and exit codes.

```typescript
const result = await sandbox.exec('python --version');
console.log(result.stdout);   // "Python 3.11.0"
console.log(result.exitCode); // 0
console.log(result.success);  // true
```

With options (environment, working directory, timeout, stdin):

```typescript
const process = await sandbox.startProcess("node server.js", {
  cwd: "/workspace/api",
  env: {
    NODE_ENV: "production",
    PORT: "8080",
    API_KEY: env.API_KEY,
    DATABASE_URL: env.DATABASE_URL,
  },
});
```

Shell commands with pipes, redirection, chaining:

```typescript
// Pipes and filters
const result = await sandbox.exec('ls -la | grep ".py" | wc -l');
console.log('Python files:', result.stdout.trim());

// Output redirection
await sandbox.exec('python generate.py > output.txt 2> errors.txt');

// Multiple commands
await sandbox.exec('cd /workspace && npm install && npm test');
```

Python execution:

```typescript
// Run inline Python
const result = await sandbox.exec('python -c "print(sum([1, 2, 3, 4, 5]))"');
console.log('Sum:', result.stdout.trim()); // "15"

// Run a script file
await sandbox.writeFile('/workspace/analyze.py', `
import sys
print(f"Argument: {sys.argv[1]}")
`);

await sandbox.exec('python /workspace/analyze.py data.csv');
```

Safe argument passing (preventing injection):

```typescript
// Safe - use proper escaping or validation
const safeFilename = filename.replace(/[^a-zA-Z0-9_.-]/g, '');
await sandbox.exec(`cat ${safeFilename}`);

// Better - write to file and execute
await sandbox.writeFile('/tmp/input.txt', userInput);
await sandbox.exec('python process.py /tmp/input.txt');
```

---

## Streaming Command Execution (execStream)

**Docs:** https://developers.cloudflare.com/sandbox/api/commands/

Executes commands returning Server-Sent Events streams emitting `start`, `stdout`, `stderr`, `complete`, and `error` events for real-time processing.

```typescript
import { parseSSEStream, type ExecEvent } from '@cloudflare/sandbox';

const stream = await sandbox.execStream('npm run build');

for await (const event of parseSSEStream<ExecEvent>(stream)) {
  switch (event.type) {
    case 'stdout':
      console.log('Output:', event.data);
      break;
    case 'complete':
      console.log('Exit code:', event.exitCode);
      break;
    case 'error':
      console.error('Failed:', event.error);
      break;
  }
}
```

With stdin input:

```typescript
const inputStream = await sandbox.execStream('python -c "import sys; print(sys.stdin.read())"', {
  stdin: 'Data from Workers!'
});

for await (const event of parseSSEStream<ExecEvent>(inputStream)) {
  if (event.type === 'stdout') {
    console.log('Python received:', event.data);
  }
}
```

---

## Background Processes (startProcess)

**Docs:** https://developers.cloudflare.com/sandbox/api/commands/ and https://developers.cloudflare.com/sandbox/guides/background-processes/

Start long-running background processes like web servers, databases, and services.

```typescript
const server = await sandbox.startProcess("python -m http.server 8000");
```

With configuration:

```typescript
const process = await sandbox.startProcess("node server.js", {
  cwd: "/workspace/api",
  env: {
    NODE_ENV: "production",
    PORT: "8080",
    API_KEY: env.API_KEY,
    DATABASE_URL: env.DATABASE_URL,
  },
});
```

### Process Management

```typescript
// List running processes
const processes = await sandbox.listProcesses();

// Kill a specific process
await sandbox.killProcess(server.id);
await sandbox.killProcess(server.id, "SIGKILL");

// Kill all processes
await sandbox.killAllProcesses();
```

### Process Readiness

```typescript
// Wait for port to be listening
await server.waitForPort(3000);

// Wait for specific log output
await server.waitForLog("Server listening");

// Wait for process to exit
const exitCode = await server.waitForExit();
```

### Process Logs

```typescript
// Stream real-time logs
const logStream = await sandbox.streamProcessLogs(server.id);
for await (const log of parseSSEStream(logStream)) {
  console.log(log.data);
}

// Get accumulated logs
const logs = await sandbox.getProcessLogs(server.id);
```

### Multiple Dependent Services

```typescript
const db = await sandbox.startProcess("redis-server");
await db.waitForPort(6379, { mode: "tcp" });

const api = await sandbox.startProcess("node api-server.js", {
  env: { DATABASE_URL: "redis://localhost:6379" },
});
await api.waitForPort(8080, { path: "/health" });
```

---

## File System Access

**Docs:** https://developers.cloudflare.com/sandbox/api/files/ and https://developers.cloudflare.com/sandbox/guides/manage-files/

### Write Files

```typescript
await sandbox.writeFile("/workspace/app.js", `console.log('Hello from sandbox!');`);
```

Supports base64 encoding for binary files.

### Read Files

```typescript
const file = await sandbox.readFile("/workspace/hello.txt");
// Returns FileInfo with content and encoding properties
```

### File Existence Check

```typescript
const result = await sandbox.exists("/workspace/app.js");
// Returns { exists: boolean }
```

### Directory Operations

```typescript
// Create directory (non-recursive by default)
await sandbox.mkdir("/workspace/src");

// Create nested directories
await sandbox.mkdir("/workspace/src/components", { recursive: true });
```

### File Management

```typescript
// Delete a file
await sandbox.deleteFile("/workspace/old.txt");

// Rename a file
await sandbox.renameFile("/workspace/old.txt", "new.txt");

// Move a file
await sandbox.moveFile("/workspace/file.txt", "/workspace/archive/");
```

### Git Checkout

```typescript
// Basic clone
await sandbox.gitCheckout("https://github.com/user/repo");

// Clone specific branch
await sandbox.gitCheckout("https://github.com/user/repo", {
  branch: "develop",
});

// Shallow clone (faster for large repos)
await sandbox.gitCheckout("https://github.com/user/large-repo", {
  depth: 1,
});

// Clone to specific directory
await sandbox.gitCheckout("https://github.com/user/my-app", {
  targetDir: "/workspace/project",
});
```

### Filesystem Structure

Standard Linux directories:
- `/workspace` -- default working directory
- `/tmp` -- temporary files
- `/home` -- user home directory
- `/usr/bin`, `/usr/local/bin` -- executable binaries

---

## File Watching

**Docs:** https://developers.cloudflare.com/sandbox/api/file-watching/ and https://developers.cloudflare.com/sandbox/guides/file-watching/

Real-time filesystem monitoring using Linux's inotify. Supports filtering, exclusions, and event callbacks.

```typescript
const watcher = await sandbox.watch("/workspace/src", {
  onEvent: (event) => {
    console.log(`${event.type} event: ${event.path}`);
    console.log(`Is directory: ${event.isDirectory}`);
  },
  onError: (error) => {
    console.error("Watch failed:", error.message);
  },
});

// Stop watching
watcher.stop();
```

### Watch Options

- `recursive` -- watch subdirectories
- `include` -- glob patterns to include (e.g., `*.ts`, `*.tsx`)
- `exclude` -- glob patterns to exclude
- `signal` -- AbortSignal for cancellation

### Event Types

- `create` -- files/directories created
- `modify` -- content or attributes changed
- `delete` -- files/directories removed
- `rename` -- files/directories moved/renamed

Default exclusions: `.git`, `node_modules`, `.DS_Store`

### Raw Stream Access

```typescript
const stream = await sandbox.watchStream(path, options);
```

### Managing Watches

```typescript
// List active watches
const result = await sandbox.listWatches();

// Stop a specific watch
await sandbox.stopWatch(watchId);
```

---

## Code Interpreter

**Docs:** https://developers.cloudflare.com/sandbox/api/interpreter/ and https://developers.cloudflare.com/sandbox/guides/code-execution/

Execute Python, JavaScript, and TypeScript code with persistent state between executions. Supports rich outputs like charts, tables, and images.

### Create Execution Context

```typescript
const context = await sandbox.createCodeContext({
  language: "python",  // or "javascript", "typescript"
  cwd: "/workspace",
  env: { API_KEY: "..." },
  timeout: 30000,  // milliseconds
});
```

### Run Code

```typescript
const result = await sandbox.runCode("print('Hello')", { context: context.id });
// Returns: success, output, outputs (rich), error
```

### State Persistence

Variables, imports, and functions persist across multiple executions within the same context. You can import libraries once then reuse them.

### Rich Output Formats

Supports: text, HTML, PNG, JPEG, SVG, LaTeX, Markdown, JSON, charts, and data tables.

### JavaScript/TypeScript Features

- Top-level `await` support
- Persistent variables across executions

### Context Management

```typescript
// List active contexts
const contexts = await sandbox.listCodeContexts();

// Delete a context
await sandbox.deleteCodeContext(contextId);
```

---

## Sessions (Shell Execution Contexts)

**Docs:** https://developers.cloudflare.com/sandbox/api/sessions/ and https://developers.cloudflare.com/sandbox/concepts/sessions/

Sessions are bash shell execution contexts within a sandbox -- like terminal tabs. Each session maintains its own shell state, environment variables, and working directory.

### Default Session

Every sandbox has a default session. Shell state (working directory, environment variables) persists across commands until the container restarts.

### Create Custom Sessions

```typescript
const buildSession = await sandbox.createSession({
  id: "build",
  env: { NODE_ENV: "production" },
  cwd: "/build"
});
```

### Get Existing Session

```typescript
const session = await sandbox.getSession("build");
```

### Delete Session

```typescript
await sandbox.deleteSession("build");
// Cannot delete the "default" session
// Deletion terminates running commands immediately
```

### Session Isolation

Each session maintains its own:
- Shell environment and exported variables
- Working directory
- Environment variables

All sessions share:
- The filesystem
- Running processes

### Set Environment Variables

```typescript
await sandbox.setEnvVars({
  NODE_ENV: "production",
  API_KEY: env.API_KEY,
});

// Unset by passing undefined/null
await sandbox.setEnvVars({
  OLD_VAR: undefined,
});
```

Environment variable precedence (highest to lowest):
1. Command-level (via `exec()` options)
2. Sandbox or session-level
3. Container default
4. System default

---

## Preview URLs (Port Exposure)

**Docs:** https://developers.cloudflare.com/sandbox/api/ports/ and https://developers.cloudflare.com/sandbox/concepts/preview-urls/ and https://developers.cloudflare.com/sandbox/guides/expose-services/

Public HTTPS access to services running inside sandboxes. When you expose a port, you get a unique URL that proxies requests to your service.

### Expose a Port

```typescript
const exposed = await sandbox.exposePort(8080, {
  hostname: 'example.com',    // Required, cannot be .workers.dev
  name: 'api',                // Optional friendly name
  token: 'api-v1',            // Optional custom token (1-16 chars)
});
console.log(exposed.url);
// https://8080-sandbox-id-api-v1.example.com
```

### URL Formats

- **Production:** `https://{port}-{sandbox-id}-{token}.yourdomain.com`
- **Auto-generated token:** `https://8080-abc123-random16chars12.yourdomain.com`
- **Custom token:** `https://8080-abc123-my-api-v1.yourdomain.com`
- **Local development:** `http://localhost:8787/...`

### Proxy to Sandbox (Required in Worker)

```typescript
import { getSandbox, proxyToSandbox } from '@cloudflare/sandbox';

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    // Must be called first to handle preview URL routing
    const proxyResponse = await proxyToSandbox(request, env);
    if (proxyResponse) return proxyResponse;

    // ... application routes
  }
};
```

### Unexpose and List Ports

```typescript
// Remove an exposed port
await sandbox.unexposePort(8000);

// List currently exposed ports
const { ports, count } = await sandbox.getExposedPorts();
for (const port of ports) {
  console.log(`  Port ${port.port}: ${port.url}`);
}
```

### Validate Port Token

```typescript
const isValid = await sandbox.validatePortToken(8080, "some-token");
```

### WebSocket Connections to Exposed Ports

```typescript
const response = await sandbox.wsConnect(request, 8080);
```

### Supported Features

- HTTP/HTTPS requests
- WebSocket connections
- Server-Sent Events
- All HTTP methods
- Request and response headers

### Unsupported

- Raw TCP/UDP connections
- Custom protocols (require HTTP wrapping)
- Ports outside 1024-65535 range
- Port 3000 (reserved for SDK internal use)

### Custom Tokens

Tokens must be 1-16 characters, lowercase letters (a-z), numbers (0-9), hyphens (-), underscores (_) only, unique per sandbox.

```typescript
const stable = await sandbox.exposePort(8080, {
  hostname,
  token: 'api-v1'
});
// https://8080-sandbox-id-api-v1.yourdomain.com
```

### Multiple Services

```typescript
const { hostname } = new URL(request.url);

await sandbox.startProcess('node api.js', { env: { PORT: '8080' } });
await new Promise(resolve => setTimeout(resolve, 2000));
const api = await sandbox.exposePort(8080, {
  hostname,
  name: 'api',
  token: 'api-prod'
});

await sandbox.startProcess('npm run dev', { env: { PORT: '5173' } });
await new Promise(resolve => setTimeout(resolve, 2000));
const frontend = await sandbox.exposePort(5173, {
  hostname,
  name: 'frontend',
  token: 'web-app'
});
```

---

## Terminal Connections (Interactive Shells)

**Docs:** https://developers.cloudflare.com/sandbox/api/terminal/ and https://developers.cloudflare.com/sandbox/concepts/terminal/ and https://developers.cloudflare.com/sandbox/guides/browser-terminals/

WebSocket-based terminal connections for interactive browser-based shells.

### Server-side Terminal Proxy

```typescript
// Default session
return await sandbox.terminal(request);

// With options
return await sandbox.terminal(request, { cols: 120, rows: 30 });

// Specific session
const session = await sandbox.getSession("dev");
return await session.terminal(request);
```

### PtyOptions

```typescript
interface PtyOptions {
  cols?: number;  // Terminal width (default: 80)
  rows?: number;  // Terminal height (default: 24)
}
```

### Client-side xterm.js Integration

```typescript
import { Terminal } from "@xterm/xterm";
import { SandboxAddon } from "@cloudflare/sandbox/xterm";

const terminal = new Terminal({ cursorBlink: true });
terminal.open(document.getElementById("terminal"));

const addon = new SandboxAddon({
  getWebSocketUrl: ({ sandboxId, sessionId, origin }) => {
    const params = new URLSearchParams({ id: sandboxId });
    if (sessionId) params.set("session", sessionId);
    return `${origin}/ws/terminal?${params}`;
  },
  onStateChange: (state, error) => {
    console.log(`Terminal ${state}`, error);
  },
});

terminal.loadAddon(addon);
addon.connect({ sandboxId: "my-sandbox" });
```

### WebSocket Protocol

- Binary frames for I/O (UTF-8 encoded keystrokes and ANSI terminal output)
- JSON text frames for control messages

Control messages (client to server):

```json
{ "type": "resize", "cols": 120, "rows": 30 }
```

Status messages (server to client):

```json
{ "type": "ready" }
{ "type": "exit", "code": 0, "signal": "SIGTERM" }
{ "type": "error", "message": "Session not found" }
```

### Output Buffering and Reconnection

The container uses a ring buffer for terminal output. When clients disconnect and reconnect, the server replays buffered output so the terminal appears unchanged. The SandboxAddon implements automatic reconnection with exponential backoff.

### Key Differences from exec()

| Use Case | Approach |
|----------|----------|
| Command execution with result retrieval | `exec()` or `execStream()` |
| Interactive end-user shell | `terminal()` |
| Long-running process with real-time output | `startProcess()` + `streamProcessLogs()` |
| Shared collaborative terminal | `terminal()` with shared session |

---

## Backup and Restore

**Docs:** https://developers.cloudflare.com/sandbox/api/backups/ and https://developers.cloudflare.com/sandbox/guides/backup-restore/

Point-in-time snapshots of sandbox directories using squashfs compression, stored in R2 buckets. Restored via copy-on-write FUSE overlays.

**Important:** Does not work with `wrangler dev`. Requires R2 bucket configuration.

### Create Backup

```typescript
const backup = await sandbox.createBackup({ dir: "/workspace" });
```

Options:
- `dir` -- required, absolute path to backup
- `name` -- optional, max 256 characters
- `ttl` -- optional, default 259200 seconds (3 days)

### Restore Backup

```typescript
const result = await sandbox.restoreBackup(backup);
```

Restores use FUSE overlayfs with copy-on-write semantics. New writes go to a writable upper layer without modifying original backups. FUSE mounts are ephemeral and lost when the sandbox sleeps or restarts.

### Requirements

1. Create R2 bucket: `npx wrangler r2 bucket create my-backup-bucket`
2. Configure `BACKUP_BUCKET` R2 binding and R2 presigned URL credentials
3. Set R2 API credentials as secrets

### Key Constraints

- Path permissions must be readable via `mksquashfs`; use `chmod -R a+rX` if needed
- TTL enforced at restore time only; expired archives remain in R2
- Concurrent operations on the same sandbox are serialized
- FUSE mounts are ephemeral (lost on sleep/restart)

---

## Storage (S3-Compatible Bucket Mounting)

**Docs:** https://developers.cloudflare.com/sandbox/api/storage/ and https://developers.cloudflare.com/sandbox/guides/mount-buckets/

Mount S3-compatible object storage (R2, S3, GCS, etc.) as local filesystems using s3fs-fuse.

**Important:** Does not work with `wrangler dev`. Requires FUSE support only available in deployed containers.

### Mount a Bucket

```typescript
await sandbox.mountBucket('my-r2-bucket', '/data', {
  endpoint: 'https://YOUR_ACCOUNT_ID.r2.cloudflarestorage.com'
});
```

### Mount Options

```typescript
interface MountBucketOptions {
  endpoint: string;          // Required - S3-compatible URL
  provider?: 'r2' | 's3' | 'gcs';  // Optional provider hint
  credentials?: {
    accessKeyId: string;
    secretAccessKey: string;
  };
  readOnly?: boolean;        // Read-only mount
  prefix?: string;           // Subdirectory within bucket (must start/end with /)
  s3fsOptions?: string;      // Advanced mount flags
}
```

### Unmount

```typescript
await sandbox.unmountBucket('/data');
// Mounted buckets are automatically unmounted on sandbox destroy
```

### Provider Examples

**Cloudflare R2:**

```typescript
await sandbox.mountBucket('my-r2-bucket', '/data', {
  endpoint: 'https://YOUR_ACCOUNT_ID.r2.cloudflarestorage.com'
});
```

**Amazon S3:**

```typescript
await sandbox.mountBucket('my-s3-bucket', '/data', {
  endpoint: 'https://s3.us-west-2.amazonaws.com',
  credentials: {
    accessKeyId: env.AWS_ACCESS_KEY_ID,
    secretAccessKey: env.AWS_SECRET_ACCESS_KEY
  }
});
```

**Google Cloud Storage (requires HMAC keys):**

```typescript
await sandbox.mountBucket('my-gcs-bucket', '/data', {
  endpoint: 'https://storage.googleapis.com',
  credentials: {
    accessKeyId: env.GCS_ACCESS_KEY_ID,
    secretAccessKey: env.GCS_SECRET_ACCESS_KEY
  }
});
```

### Prefix Mounting (Subdirectories)

```typescript
await sandbox.mountBucket('datasets', '/training-data', {
  endpoint: 'https://YOUR_ACCOUNT_ID.r2.cloudflarestorage.com',
  prefix: '/ml/training/'
});

await sandbox.mountBucket('datasets', '/test-data', {
  endpoint: 'https://YOUR_ACCOUNT_ID.r2.cloudflarestorage.com',
  prefix: '/ml/testing/'
});
```

### Read-Only Mounts

```typescript
await sandbox.mountBucket('dataset-bucket', '/data', {
  endpoint: 'https://YOUR_ACCOUNT_ID.r2.cloudflarestorage.com',
  readOnly: true
});
```

### Credential Resolution Order

1. Explicit credentials in options
2. Environment variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`)
3. Automatic R2 bucket detection

---

## Transport Modes

**Docs:** https://developers.cloudflare.com/sandbox/configuration/transport/

Two communication modes between the SDK and container:

### HTTP Transport (Default)

Each SDK operation (`exec()`, `readFile()`, `writeFile()`) makes a separate HTTP request to the container API. Each operation counts as one subrequest.

### WebSocket Transport

Multiplexes all SDK operations over a single persistent WebSocket connection. The WebSocket upgrade counts as one subrequest regardless of how many operations you perform.

**Configuration:**

```json
{
  "vars": {
    "SANDBOX_TRANSPORT": "websocket"
  }
}
```

**Subrequest limits:**
- Workers Free: 50 subrequests per request
- Workers Paid: 1,000 subrequests per request

**HTTP Transport (4 subrequests):**

```typescript
await sandbox.exec("python setup.py");
await sandbox.writeFile("/app/config.json", config);
await sandbox.exec("python process.py");
const result = await sandbox.readFile("/app/output.txt");
```

**WebSocket Transport (1 subrequest):**

```typescript
// Identical code - transport is configured via environment variable
await sandbox.exec("python setup.py");
await sandbox.writeFile("/app/config.json", config);
await sandbox.exec("python process.py");
const result = await sandbox.readFile("/app/output.txt");
```

| Scenario | Recommended |
|----------|-------------|
| Many SDK operations per request | WebSocket |
| Running inside Workers or Durable Objects | WebSocket |
| Approaching subrequest limits | WebSocket |
| Simple, infrequent sandbox usage | HTTP (default) |
| Debugging individual requests | HTTP (default) |

---

## Dockerfile Configuration (Container Images)

**Docs:** https://developers.cloudflare.com/sandbox/configuration/dockerfile/

### Base Image Variants

| Image | Tag | Purpose |
|-------|-----|---------|
| Default | (none) | Lean image for JavaScript/TypeScript workloads |
| Python | `-python` | Data science, ML, Python code execution |
| OpenCode | `-opencode` | AI coding agents with OpenCode CLI |

```dockerfile
FROM docker.io/cloudflare/sandbox:0.7.0
FROM docker.io/cloudflare/sandbox:0.7.0-python
FROM docker.io/cloudflare/sandbox:0.7.0-opencode
```

**Critical:** Always match the Docker image version to your npm package version.

### Default Image Contents

- Ubuntu 22.04 LTS
- Node.js 20 LTS with npm
- Bun 1.x runtime
- System tools: curl, wget, git, jq, zip, unzip, file, procps, ca-certificates

### Python Variant Additions

- Python 3.11 with pip/venv
- Pre-installed: matplotlib, numpy, pandas, ipython

### Custom Images

You can extend images or use arbitrary base images by copying the sandbox binary from official images:

```dockerfile
FROM docker.io/cloudflare/sandbox:0.7.0

# Add Python packages
RUN pip install scikit-learn tensorflow transformers

# Add Node.js packages
RUN npm install -g typescript ts-node prettier

# Add system packages
RUN apt-get update && apt-get install -y postgresql-client redis-tools
```

### Dynamic Package Installation at Runtime

```bash
pip install scikit-learn tensorflow
npm install express
apt-get update && apt-get install -y redis-server
```

---

## Wrangler Configuration

**Docs:** https://developers.cloudflare.com/sandbox/configuration/wrangler/

### Minimal wrangler.jsonc

```json
{
  "name": "my-sandbox-worker",
  "main": "src/index.ts",
  "compatibility_date": "2026-02-25",
  "compatibility_flags": ["nodejs_compat"],
  "containers": [
    {
      "class_name": "Sandbox",
      "image": "./Dockerfile",
      "instance_type": "lite",
      "max_instances": 1
    }
  ],
  "durable_objects": {
    "bindings": [
      {
        "class_name": "Sandbox",
        "name": "Sandbox"
      }
    ]
  },
  "migrations": [
    {
      "new_sqlite_classes": ["Sandbox"],
      "tag": "v1"
    }
  ]
}
```

### Minimal wrangler.toml

```toml
name = "my-sandbox-worker"
main = "src/index.ts"
compatibility_date = "2026-02-25"
compatibility_flags = ["nodejs_compat"]

[[containers]]
class_name = "Sandbox"
image = "./Dockerfile"
instance_type = "lite"
max_instances = 1

[[durable_objects.bindings]]
class_name = "Sandbox"
name = "Sandbox"

[[migrations]]
new_sqlite_classes = [ "Sandbox" ]
tag = "v1"
```

### Backup Storage Configuration

```bash
npx wrangler r2 bucket create my-backup-bucket
npx wrangler secret put R2_ACCESS_KEY_ID
npx wrangler secret put R2_SECRET_ACCESS_KEY
```

---

## Environment Variables

**Docs:** https://developers.cloudflare.com/sandbox/configuration/environment-variables/

### SDK Configuration Variables

| Variable | Values | Default | Description |
|----------|--------|---------|-------------|
| `SANDBOX_TRANSPORT` | `"http"`, `"websocket"` | `"http"` | Communication protocol |
| `SANDBOX_LOG_LEVEL` | `"debug"`, `"info"`, `"warn"`, `"error"` | `"info"` | Log verbosity |
| `SANDBOX_LOG_FORMAT` | `"json"`, `"pretty"` | `"json"` | Log format |
| `SANDBOX_INSTANCE_TIMEOUT_MS` | number | `30000` | Provisioning wait timeout |
| `SANDBOX_PORT_TIMEOUT_MS` | number | `90000` | API readiness timeout |

### Three Methods for Setting Variables

**Method 1: Sandbox-level (global)**

```typescript
await sandbox.setEnvVars({
  NODE_ENV: "production",
  API_KEY: env.API_KEY,
});
```

**Method 2: Per-command**

```typescript
const result = await sandbox.exec("node app.js", {
  env: { DEBUG: "true" }
});
```

**Method 3: Session-level**

```typescript
const session = await sandbox.createSession({
  id: "build",
  env: { NODE_ENV: "production" },
  cwd: "/build"
});
```

### Passing Worker Secrets

```bash
wrangler secret put OPENAI_API_KEY
wrangler secret put DATABASE_URL
```

---

## Sandbox Options (Container Timeouts)

**Docs:** https://developers.cloudflare.com/sandbox/configuration/sandbox-options/

```typescript
const sandbox = getSandbox(env.Sandbox, 'data-processor', {
  containerTimeouts: {
    instanceGetTimeoutMS: 30000,    // Provisioning wait (default: 30000)
    portReadyTimeoutMS: 180_000     // 3 minutes for heavy startup work
  }
});
```

### Normalize ID (DNS Compatibility)

```typescript
const sandbox1 = getSandbox(env.Sandbox, 'MyProject-123');
// Durable Object ID: "MyProject-123"

const sandbox2 = getSandbox(env.Sandbox, 'MyProject-123', {
  normalizeId: true
});
// Durable Object ID: "myproject-123"
```

Required for preview URLs when sandbox IDs contain uppercase letters (hostnames are case-insensitive per RFC 3986).

---

## Security Model

**Docs:** https://developers.cloudflare.com/sandbox/concepts/security/

### Isolation Between Sandboxes

Each sandbox runs in a separate VM with complete isolation:
- Filesystem isolation
- Process isolation
- Network isolation
- Resource limit enforcement

### Isolation Within Sandboxes

All code within a single sandbox shares resources. The docs advise: **use separate sandboxes per user** to prevent unauthorized file access between sessions.

### Developer Responsibilities

The SDK protects against:
- Sandbox-to-sandbox access
- Resource exhaustion
- Container escapes

Developers must implement:
- Authentication and authorization
- Input validation and sanitization
- Rate limiting
- Application-level security

### Preview URL Security

- Preview URLs are public by default; anyone with the URL can access the service
- Auto-generated tokens are random 16 characters
- Tokens regenerate when ports are unexposed and re-exposed
- Use `unexposePort()` to revoke access

---

## Container Runtime Details

**Docs:** https://developers.cloudflare.com/sandbox/concepts/containers/

### Network Capabilities

- Outbound connections work normally
- Inbound connections require port exposure via `exposePort()`
- Localhost communication between processes within the same sandbox is supported

### Limitations

- Cannot load kernel modules
- Cannot access host hardware
- Port 3000 is reserved for the SDK's internal Bun server

---

## Git Workflows

**Docs:** https://developers.cloudflare.com/sandbox/guides/git-workflows/

### Clone Private Repositories

```typescript
const token = env.GITHUB_TOKEN;
const repoUrl = `https://${token}@github.com/user/private-repo.git`;
await sandbox.gitCheckout(repoUrl);
```

### Clone and Build

```typescript
await sandbox.gitCheckout("https://github.com/user/my-app");
const repoName = "my-app";
await sandbox.exec(`cd ${repoName} && npm install`);
await sandbox.exec(`cd ${repoName} && npm run build`);
```

### Branch Operations and Commits

```typescript
await sandbox.gitCheckout("https://github.com/user/repo");
await sandbox.exec("cd repo && git checkout feature-branch");
await sandbox.exec("cd repo && git checkout -b new-feature");

// Make changes and commit
const readme = await sandbox.readFile("/workspace/repo/README.md");
await sandbox.writeFile(
  "/workspace/repo/README.md",
  readme.content + "\n\n## New Section",
);
await sandbox.exec('cd repo && git config user.name "Sandbox Bot"');
await sandbox.exec('cd repo && git config user.email "bot@example.com"');
await sandbox.exec("cd repo && git add README.md");
await sandbox.exec('cd repo && git commit -m "Update README"');
```

---

## Docker-in-Docker

**Docs:** https://developers.cloudflare.com/sandbox/guides/docker-in-docker/

Run Docker commands within a sandbox container. Uses `FROM docker:dind-rootless` base image with the sandbox binary copied in.

### Dockerfile Setup

```dockerfile
FROM docker:dind-rootless

USER root

# Use the musl build so it runs on Alpine-based docker:dind-rootless
COPY --from=docker.io/cloudflare/sandbox:0.7.4-musl /container-server/sandbox /sandbox
COPY --from=docker.io/cloudflare/sandbox:0.7.4-musl /usr/lib/libstdc++.so.6 /usr/lib/libstdc++.so.6
COPY --from=docker.io/cloudflare/sandbox:0.7.4-musl /usr/lib/libgcc_s.so.1 /usr/lib/libgcc_s.so.1
COPY --from=docker.io/cloudflare/sandbox:0.7.4-musl /bin/bash /bin/bash
COPY --from=docker.io/cloudflare/sandbox:0.7.4-musl /usr/lib/libreadline.so.8 /usr/lib/libreadline.so.8
COPY --from=docker.io/cloudflare/sandbox:0.7.4-musl /usr/lib/libreadline.so.8.2 /usr/lib/libreadline.so.8.2

RUN printf '#!/bin/sh\n\
  set -eu\n\
  dockerd-entrypoint.sh dockerd --iptables=false --ip6tables=false &\n\
  until docker version >/dev/null 2>&1; do sleep 0.2; done\n\
  echo "Docker is ready"\n\
  wait\n' > /home/rootless/boot-docker-for-dind.sh && chmod +x /home/rootless/boot-docker-for-dind.sh

ENTRYPOINT ["/sandbox"]
CMD ["/home/rootless/boot-docker-for-dind.sh"]
```

### Usage

```typescript
import { getSandbox } from "@cloudflare/sandbox";

const sandbox = getSandbox(env.Sandbox, "docker-sandbox");

// Build an image
await sandbox.writeFile(
  "/workspace/Dockerfile",
  `
FROM alpine:latest
RUN apk add --no-cache curl
CMD ["echo", "Hello from Docker!"]
`,
);

const build = await sandbox.exec(
  "docker build --network=host -t my-image /workspace",
);
if (!build.success) {
  console.error("Build failed:", build.stderr);
}

// Run a container
const run = await sandbox.exec("docker run --network=host --rm my-image");
console.log(run.stdout); // "Hello from Docker!"
```

### Key Constraints

- Must use `--network=host` flag for Docker commands (iptables not supported)
- Rootless mode only
- Built images and containers are lost when the sandbox sleeps (ephemeral storage)

---

## WebSocket Connections to Sandbox Services

**Docs:** https://developers.cloudflare.com/sandbox/guides/websocket-connections/

Two approaches for connecting to WebSocket servers running in sandboxes:

1. **Preview URL approach:** Expose port and convert HTTPS to WSS URL
2. **Worker-to-Sandbox via wsConnect():** Direct WebSocket proxying

```typescript
// Worker checks for WebSocket upgrade and proxies to sandbox
const response = await sandbox.wsConnect(request, 8080);
```

---

## Streaming Output

**Docs:** https://developers.cloudflare.com/sandbox/guides/streaming-output/

Real-time output from commands via `execStream()` and `streamProcessLogs()`.

Use streaming for:
- Long-running operations (builds, installations)
- Interactive applications (chatbots)
- Large output processed incrementally

Use non-streaming `exec()` for:
- Quick operations completing in seconds
- Small output fitting in memory

SSE events emitted: `start`, `stdout`, `stderr`, `complete`, `error`.

### Server-Side Streaming Response

```typescript
import { getSandbox } from '@cloudflare/sandbox';

export { Sandbox } from '@cloudflare/sandbox';

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const sandbox = getSandbox(env.Sandbox, 'builder');
    const stream = await sandbox.execStream('npm run build');

    return new Response(stream, {
      headers: {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache'
      }
    });
  }
};
```

### Client-Side EventSource Consumption

```typescript
const eventSource = new EventSource('/build');

eventSource.addEventListener('stdout', (event) => {
  const data = JSON.parse(event.data);
  console.log(data.data);
});

eventSource.addEventListener('complete', (event) => {
  const data = JSON.parse(event.data);
  console.log('Exit code:', data.exitCode);
  eventSource.close();
});
```

### Streaming Process Logs

```typescript
import { parseSSEStream, type LogEvent } from '@cloudflare/sandbox';

const process = await sandbox.startProcess('node server.js');
const logStream = await sandbox.streamProcessLogs(process.id);

for await (const log of parseSSEStream<LogEvent>(logStream)) {
  console.log(log.data);

  if (log.data.includes('Server listening')) {
    console.log('Server is ready');
    break;
  }
}
```

---

## Production Deployment (Custom Domains)

**Docs:** https://developers.cloudflare.com/sandbox/guides/production-deployment/

Required **only** if using `exposePort()`. Applications without exposed ports can deploy to `.workers.dev`.

### Requirements

- Active Cloudflare zone with custom domain
- Wildcard DNS record (A record, Name: `*`, IPv4: `192.0.2.0`, Proxied)
- Wildcard route in wrangler config

### TLS Certificate Options

Cloudflare Universal SSL only covers first-level wildcards (`*.yourdomain.com`). Options:

1. Deploy on apex domain (simplest, uses Universal SSL)
2. Advanced Certificate Manager ($10/month)
3. Upload custom certificates from Let's Encrypt (free, requires renewal)

### Wrangler Route Configuration

```json
{
  "routes": [
    {
      "pattern": "*.yourdomain.com/*",
      "zone_name": "yourdomain.com"
    }
  ]
}
```

### Verification

```typescript
const { hostname } = new URL(request.url);
const sandbox = getSandbox(env.Sandbox, 'test-sandbox');
await sandbox.startProcess('python -m http.server 8080');
const exposed = await sandbox.exposePort(8080, { hostname });
console.log(exposed.url);
// https://8080-test-sandbox.yourdomain.com
```

---

## Logging Configuration

**Docs:** https://developers.cloudflare.com/sandbox/configuration/sandbox-options/

```json
{
  "vars": {
    "SANDBOX_LOG_LEVEL": "debug",
    "SANDBOX_LOG_FORMAT": "pretty"
  }
}
```

Levels: `debug`, `info`, `warn`, `error` (default: `info`)
Formats: `json`, `pretty` (default: `json`)

---

## Instance Types and Resource Limits

**Docs:** https://developers.cloudflare.com/sandbox/platform/limits/ (references https://developers.cloudflare.com/containers/platform-details/limits/)

### Predefined Instance Types

| Instance Type | vCPU | Memory | Disk |
|---------------|------|--------|------|
| lite | 1/16 | 256 MiB | 2 GB |
| basic | 1/4 | 1 GiB | 4 GB |
| standard-1 | 1/2 | 4 GiB | 8 GB |
| standard-2 | 1 | 6 GiB | 12 GB |
| standard-3 | 2 | 8 GiB | 16 GB |
| standard-4 | 4 | 12 GiB | 20 GB |

Legacy aliases: `dev` maps to `lite`, `standard` maps to `standard-1`.

### Custom Instance Type Constraints

| Resource | Limit |
|----------|-------|
| Minimum vCPU | 1 |
| Maximum vCPU | 4 |
| Maximum Memory | 12 GiB |
| Maximum Disk | 20 GB |
| Memory to vCPU ratio | Minimum 3 GiB per vCPU |
| Disk to Memory ratio | Maximum 2 GB per 1 GiB memory |

### Account-Level Limits (Open Beta)

| Resource | Limit |
|----------|-------|
| Memory across concurrent instances | 6 TiB |
| vCPU allocation | 1,500 |
| Total disk storage | 30 TB |
| Individual image size | Matches instance disk space |
| Account image storage quota | 50 GB total |

### Worker Subrequest Limits

| Plan | Subrequests per Request |
|------|------------------------|
| Workers Free | 50 |
| Workers Paid | 1,000 |

Each SDK operation counts as one subrequest under HTTP transport. WebSocket transport uses only one subrequest for the connection upgrade.

---

## Pricing

**Docs:** https://developers.cloudflare.com/sandbox/platform/pricing/ (references https://developers.cloudflare.com/containers/pricing/)

Sandbox pricing is determined by the underlying Containers platform. Containers charge for every 10ms that they are actively running.

### Workers Paid Plan Includes

| Resource | Included | Overage Rate |
|----------|----------|--------------|
| Memory | 25 GiB-hours/month | $0.0000025 per GiB-second |
| CPU | 375 vCPU-minutes/month | $0.000020 per vCPU-second |
| Disk | 200 GB-hours/month | $0.00000007 per GB-second |

### Network Egress Rates

| Region | Price/GB | Monthly Included |
|--------|----------|------------------|
| North America & Europe | $0.025 | 1 TB |
| Oceania, Korea, Taiwan | $0.05 | 500 GB |
| Everywhere Else | $0.04 | 500 GB |

### Additional Billable Services

- **Workers** -- processes incoming requests
- **Durable Objects** -- powers each sandbox instance
- **Workers Logs** -- optional observability

Base Workers Paid plan: $5/month.

---

## Version Compatibility

**Docs:** https://developers.cloudflare.com/sandbox/concepts/sandboxes/

The SDK validates npm package versions against Docker container image versions. Mismatches cause warnings and can break features. Always match versions:

```dockerfile
FROM docker.io/cloudflare/sandbox:0.7.0
```

Must match npm package `@cloudflare/sandbox@0.7.0`.

---

## Request Routing and Geography

**Docs:** https://developers.cloudflare.com/sandbox/concepts/sandboxes/

First requests determine geographic location; subsequent requests route to the same location. For global applications, either use region-suffixed sandbox IDs or accept potential latency.

---

## Integration with Workers (Minimal Example)

**Docs:** https://developers.cloudflare.com/sandbox/get-started/

```typescript
import { getSandbox, proxyToSandbox, type Sandbox } from "@cloudflare/sandbox";

export { Sandbox } from "@cloudflare/sandbox";

type Env = {
  Sandbox: DurableObjectNamespace<Sandbox>;
};

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    // Get or create a sandbox instance
    const sandbox = getSandbox(env.Sandbox, "my-sandbox");

    // Execute Python code
    if (url.pathname === "/run") {
      const result = await sandbox.exec('python3 -c "print(2 + 2)"');
      return Response.json({
        output: result.stdout,
        error: result.stderr,
        exitCode: result.exitCode,
        success: result.success,
      });
    }

    // Work with files
    if (url.pathname === "/file") {
      await sandbox.writeFile("/workspace/hello.txt", "Hello, Sandbox!");
      const file = await sandbox.readFile("/workspace/hello.txt");
      return Response.json({
        content: file.content,
      });
    }

    return new Response("Try /run or /file");
  },
};
```

### Development

```bash
npm run dev
curl http://localhost:8787/run
curl http://localhost:8787/file
```

### Deployment

```bash
npx wrangler deploy
npx wrangler containers list
curl https://my-sandbox.YOUR_SUBDOMAIN.workers.dev/run
```

---

## Local Development

**Docs:** https://developers.cloudflare.com/sandbox/get-started/

- Requires Docker running locally
- Docker must be running when you run `wrangler deploy` (for image building)
- Local dev uses `wrangler dev`
- Ports must be declared with `EXPOSE` in Dockerfile for local dev access

```dockerfile
FROM docker.io/cloudflare/sandbox:0.3.3

EXPOSE 8000
EXPOSE 8080
EXPOSE 5173
```

In production, all ports are available and controlled programmatically via `exposePort()` / `unexposePort()`.

---

## Wrangler CLI Commands

**Docs:** https://developers.cloudflare.com/sandbox/get-started/

```bash
# Create project from template
npm create cloudflare@latest -- my-sandbox --template=cloudflare/sandbox-sdk/examples/minimal

# Local development
npm run dev

# Deploy
npx wrangler deploy

# List containers
npx wrangler containers list
```
