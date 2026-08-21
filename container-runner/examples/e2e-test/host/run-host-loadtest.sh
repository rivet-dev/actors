#!/usr/bin/env bash
# Host-based container load test: run the Rivet engine + N container-runner instances
# natively (no Docker), each wrapping the lightweight ping-pong test-server (NOT a Unity
# server), then drive a WebSocket ping-pong through the guard to every one.
#
# Each container-runner instance is one "container": its own front-door port, its own child
# port, and its own serverless runner pool (load-<i>). This mirrors the serverless 1:1
# actor<->container model locally, so `LOAD_COUNT` instances == that many containers.
#
#   engine (:7420 guard, :7421 api)
#   for i in 0..N-1: container-runner :$((18100+i)) -> child ping-pong :$((28100+i)), pool load-<i>
#   load-test.mjs: create N actors -> ws ping-pong each -> report success + latency
#
# Usage:  e2e-test/host/run-host-loadtest.sh
#   LOAD_COUNT       number of containers to spawn        (default 25; the assignment target is 1000)
#   LOAD_CONCURRENCY in-flight create/ping ops            (default 64)
#
# NOTE: 1000 local instances is ~2000 processes (Rust runner + Node child each) and needs a
# beefy host + raised `ulimit -n`. For a true 1000-container run, prefer Rivet Cloud (Cloud
# Run scales the containers) by pointing load-test.mjs at the cloud engine — see README.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
E2E="$(dirname "$HERE")"
ROOT="$(cd "$E2E/../../.." && pwd)"
DATA="$E2E/.host-data"
LOGS="$E2E/.host-logs/loadtest"
RUNNER_BIN="$ROOT/target/release/rivet-container-runner"
ENGINE_BIN="${ENGINE_BINARY:-$ROOT/target/debug/rivet-engine}"
GUARD_PORT="${GUARD_PORT:-7420}"
API_PORT="${API_PORT:-7421}"
FRONT_BASE="${FRONT_BASE:-18100}"
CHILD_BASE="${CHILD_BASE:-28100}"
N="${LOAD_COUNT:-25}"
export LOAD_CONCURRENCY="${LOAD_CONCURRENCY:-64}"

mkdir -p "$DATA" "$LOGS"

ENGINE_PID=""; RUNNER_PIDS=()
cleanup() {
  echo "==> Cleaning up ($(( ${#RUNNER_PIDS[@]} )) runners)"
  for pid in "${RUNNER_PIDS[@]:-}"; do [ -n "$pid" ] && kill "$pid" 2>/dev/null || true; done
  [ -n "$ENGINE_PID" ] && kill "$ENGINE_PID" 2>/dev/null || true
  sleep 1
  pkill -f "$ROOT/container-runner/examples/test-server/server.mjs" 2>/dev/null || true
}
trap cleanup EXIT

# ---- ensure binaries + deps ----
if [ ! -x "$ENGINE_BIN" ]; then
  echo "==> Building rivet-engine (debug)"
  (cd "$ROOT" && cargo build -p rivet-engine)
fi
if [ ! -x "$RUNNER_BIN" ]; then
  echo "==> Building container-runner (release)"
  (cd "$ROOT" && cargo build --release -p rivet-container-runner)
fi
[ -d "$ROOT/container-runner/examples/test-server/node_modules" ] || (cd "$ROOT/container-runner/examples/test-server" && npm install --silent)
[ -d "$E2E/node_modules" ] || (cd "$E2E" && npm install --silent)

# ---- start the engine ----
echo "==> Starting engine (guard :$GUARD_PORT, api :$API_PORT)"
rm -rf "$DATA"; mkdir -p "$DATA"
RIVET__FILE_SYSTEM__PATH="$DATA" "$ENGINE_BIN" start --config "$HERE/config.jsonc" \
  > "$LOGS/engine.log" 2>&1 &
ENGINE_PID=$!
for _ in $(seq 1 40); do curl -sf "http://127.0.0.1:${API_PORT}/health" >/dev/null 2>&1 && break; sleep 1; done
curl -sf "http://127.0.0.1:${API_PORT}/health" >/dev/null 2>&1 || { echo "engine failed"; tail -20 "$LOGS/engine.log"; exit 1; }

export ENGINE_URL="http://127.0.0.1:${GUARD_PORT}"
export ENGINE_PUBLIC_URL="http://127.0.0.1:${GUARD_PORT}"

# ---- start N container-runners, each its own pool ----
echo "==> Starting $N container-runner instances + registering pools"
for i in $(seq 0 $((N - 1))); do
  front=$((FRONT_BASE + i)); child=$((CHILD_BASE + i))
  PORT=$front CHILD_PORT=$child RIVET_ACTOR_NAME=game RUST_LOG=warn \
    "$RUNNER_BIN" -- node "$ROOT/container-runner/examples/test-server/server.mjs" > "$LOGS/runner-$i.log" 2>&1 &
  RUNNER_PIDS+=($!)
done
# wait for every front door to answer, then register its pool
for i in $(seq 0 $((N - 1))); do
  front=$((FRONT_BASE + i))
  for _ in $(seq 1 30); do curl -sf "http://127.0.0.1:${front}/api/rivet/health" >/dev/null 2>&1 && break; sleep 0.5; done
  RIVET_RUNNER_NAME="load-$i" SERVERLESS_URL="http://127.0.0.1:${front}/api/rivet" \
    node "$E2E/configure-serverless.mjs" >/dev/null
done
echo "    $N runners up, $N pools registered"

# ---- drive the load test ----
echo "==> Running load test ($N containers)"
LOAD_COUNT="$N" LOAD_POOL_PREFIX="load" node "$E2E/load-test.mjs"
RESULT=$?
echo
[ "$RESULT" = "0" ] && echo "PASS: host container load test ($N containers) succeeded." \
  || echo "FAIL: host container load test did not meet the threshold."
exit $RESULT
