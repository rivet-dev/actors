# syntax=docker/dockerfile:1
#
# Game container for local e2e: builds container-runner (linux/amd64) and runs it as
# the ENTRYPOINT wrapping the Node test server (the Unity dedicated server stand-in).
#
# Build from the RIVET REPO ROOT so the workspace and examples are in context.
# Arch-agnostic: builds for the host/requested platform.
#   Local (native, e.g. arm64 Mac): docker build -f container-runner/examples/test-server/Dockerfile -t game:latest .
#   Serverless (x86_64):            docker build --platform linux/amd64 -f container-runner/examples/test-server/Dockerfile -t game:amd64 .
#
# Production pattern (per spec) — instead of building from source, curl a released
# binary into an existing image:
#   RUN curl -fsSL https://releases.rivet.dev/rivet/latest/container-runner/rivet-container-runner-x86_64-unknown-linux-musl \
#       -o /usr/local/bin/rivet-container-runner && chmod +x /usr/local/bin/rivet-container-runner
#   ENTRYPOINT ["rivet-container-runner", "--", "<their server launch command>"]

# ---- build container-runner ----
FROM rust:1-bookworm AS builder
WORKDIR /build
# The crate depends on in-repo workspace crates (rivet-envoy-client), so the whole
# workspace is the build context. Build from the rivet repo root.
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release -p rivet-container-runner --bin rivet-container-runner \
    && cp target/release/rivet-container-runner /usr/local/bin/rivet-container-runner

# ---- runtime: node + runner + test server ----
FROM node:20-slim AS runtime
WORKDIR /app
COPY --from=builder /usr/local/bin/rivet-container-runner /usr/local/bin/rivet-container-runner
COPY container-runner/examples/test-server/ /app/test-server/
RUN cd /app/test-server && npm install --omit=dev

# The serverless platform sets $PORT (the front door). The child listens on CHILD_PORT.
ENV PORT=8080 \
    CHILD_PORT=7770 \
    RIVET_ACTOR_NAME=game
EXPOSE 8080

# The child command follows `--`. Swap `node ...` for the Unity Server.x86_64 later.
ENTRYPOINT ["rivet-container-runner", "--", "node", "/app/test-server/server.mjs"]
