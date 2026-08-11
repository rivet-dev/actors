#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

POSTGRES_IMAGE="postgres:18"

# pg_isready reports ready only once the server is actually accepting connections.
# The Postgres entrypoint binds the port during its bootstrap phase and then
# restarts, so a plain port check (nc -z) passes too early and the engine hits
# "connection reset" / "early eof" on first connect. Run pg_isready from a throwaway
# container on the host network so no client binary needs to be installed locally.
postgres_ready() {
  docker run --rm --network host "${POSTGRES_IMAGE}" \
    pg_isready -h localhost -p 5432 -U postgres -d postgres >/dev/null 2>&1
}

if ! postgres_ready; then
  echo "Postgres is not accepting connections."
  echo "Starting postgres container..."
  "${SCRIPT_DIR}/postgres.sh"

  echo "Waiting for postgres to be ready..."
  for i in {1..30}; do
    if postgres_ready; then
      echo "Postgres is ready!"
      break
    fi
    if [ $i -eq 30 ]; then
      echo "error: postgres did not become ready in time"
      exit 1
    fi
    sleep 1
  done
fi

cd "${REPO_ROOT}"

RUST_BACKTRACE=full \
RIVET__POSTGRES__URL=postgres://postgres:postgres@localhost:5432/postgres \
RUST_LOG=debug \
RUST_LOG_TARGET=1 \
cargo run --bin rivet-engine -- start 2>&1 | tee -i /tmp/rivet-engine.log
