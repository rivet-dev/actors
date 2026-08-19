# syntax=docker/dockerfile:1
#
# Production image for the Unity Linux dedicated server and FishyWebRTC signaling
# endpoint, wrapped by `rivet-container-runner`.
#
# The engine cold-starts this container and drives it over the serverless protocol
# (`POST $BASE_PATH/start` on the front-door port). The runner spawns the Unity server
# as its child and proxies Rivet's tunneled traffic to it.
#
# ---- Prerequisites -----------------------------------------------------------------
# Build the Unity Linux server FIRST (needs Unity "Linux Build Support" module):
#   container-runner/examples/unity-webrtc-demo/setup-and-build.sh linux
#   -> container-runner/examples/unity-webrtc-demo/Builds/ServerLinux/GameServer
#
# ---- Build (from the RIVET REPO ROOT so the workspace and examples are in context) --
#   docker build --platform linux/amd64 -f container-runner/examples/unity-webrtc-demo/Dockerfile \
#     --build-arg RIVET_RUNNER_VERSION=$(date +%s) -t game-unity:amd64 .
#
# ---- Run locally to smoke-test the front door --------------------------------------
#   docker run --rm -p 8080:8080 game-unity:amd64
#   curl -s localhost:8080/api/rivet/health   # -> ok
#
# ---- Deploy-time env vars (all optional; override in Rivet Compute / your host) -----
#   RIVET_PORT / PORT        Front-door HTTP port. Rivet Compute injects RIVET_PORT; the
#                            runner reads it directly (falls back to PORT). (default 8080)
#   CHILD_PORT               Local port the Unity server listens on. (default 7770)
#   RIVET_ACTOR_NAME         Actor name this runner serves. (default game)
#   RIVET_RUNNER_VERSION     Version reported to the engine for drain-on-deploy. Bump
#                            every deploy (the --build-arg above bakes a default).
#   RIVET_SERVERLESS_BASE_PATH  Serverless base path. (default /api/rivet)
#   RIVET_STOP_GRACE_SECS    SIGTERM->SIGKILL grace for the child. (default 25)
#   RIVET_READINESS_TIMEOUT_SECS  Wait for the child's port to open. (default 30)

# ---- runner: download the stable Cloud-compatible release ---------------------------
FROM --platform=linux/amd64 alpine:3.22 AS runner

RUN apk add --no-cache ca-certificates curl \
    && curl -fsSL \
        https://releases.rivet.dev/rivet/2.3.3/container-runner/rivet-container-runner-x86_64-unknown-linux-musl \
        -o /rivet-container-runner \
    && chmod +x /rivet-container-runner

# ---- runtime: Unity Linux dedicated server + runner ----------------------------------
FROM debian:bookworm-slim AS runtime

# Unity's headless dedicated server needs glibc and certificates.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /game
COPY --from=runner /rivet-container-runner /usr/local/bin/rivet-container-runner

# The Unity Linux dedicated-server build (executable + _Data + UnityPlayer.so).
# Built on the host by: container-runner/examples/unity-webrtc-demo/setup-and-build.sh linux
COPY container-runner/examples/unity-webrtc-demo/Builds/ServerLinux/ /game/
RUN chmod +x /game/GameServer

# Runner version is baked as a default; override at deploy time by setting the env var.
ARG RIVET_RUNNER_VERSION=1
ENV RIVET_RUNNER_VERSION=${RIVET_RUNNER_VERSION} \
    CHILD_PORT=7770 \
    RIVET_ACTOR_NAME=game \
    RIVET_SERVERLESS_BASE_PATH=/api/rivet
EXPOSE 8080

# Rivet Compute injects RIVET_PORT for the front door; the runner reads it directly.
# `-logFile -` sends Unity logs to stdout so they show up in the Rivet Compute logs.
ENTRYPOINT ["rivet-container-runner", "--", "./GameServer", "-batchmode", "-nographics", "-logFile", "-"]
