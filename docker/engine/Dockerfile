# syntax=docker/dockerfile:1.10.0

# MARK: Builder
# TODO(RVT-4168): Compile libfdb from scratch for ARM
FROM ghcr.io/rivet-dev/rivet/engine-base-builder:a36b881 AS builder

# Docker automatically provides TARGETARCH
ARG TARGETARCH

ARG BUILD_FRONTEND=false
ARG VITE_FEATURE_FLAGS=
ARG CARGO_BUILD_MODE=debug
ARG VITE_APP_API_URL=__SAME__
ARG VITE_APP_TURNSTILE_SITE_KEY=
ARG OVERRIDE_GIT_SHA
ARG RUST_TOOLCHAIN=1.91.1
ARG SCCACHE_VERSION=v0.8.2

# Install sccache so the engine compile shares the Depot WebDAV cache with the
# matrix builds (docker/build/*.Dockerfile). The engine base image does not
# ship it, unlike the builder-base-linux-* images. Installed before COPY so the
# layer stays cached across source changes. Match the binary to the native
# build arch (each docker-images matrix leg runs on its own arch runner).
RUN if [ "$TARGETARCH" = "arm64" ]; then SCCACHE_ARCH=aarch64; else SCCACHE_ARCH=x86_64; fi && \
    wget -q "https://github.com/mozilla/sccache/releases/download/${SCCACHE_VERSION}/sccache-${SCCACHE_VERSION}-${SCCACHE_ARCH}-unknown-linux-musl.tar.gz" && \
    tar -xzf "sccache-${SCCACHE_VERSION}-${SCCACHE_ARCH}-unknown-linux-musl.tar.gz" && \
    mv "sccache-${SCCACHE_VERSION}-${SCCACHE_ARCH}-unknown-linux-musl/sccache" /usr/local/bin/sccache && \
    chmod +x /usr/local/bin/sccache && \
    rm -rf "sccache-${SCCACHE_VERSION}-${SCCACHE_ARCH}-unknown-linux-musl"*

ENV RUSTC_WRAPPER=sccache \
    SCCACHE_WEBDAV_ENDPOINT=https://cache.depot.dev \
    SCCACHE_IDLE_TIMEOUT=0

WORKDIR /app

COPY . .

RUN rustup toolchain install "${RUST_TOOLCHAIN}" --profile minimal && \
    rustup default "${RUST_TOOLCHAIN}"

# Build frontend. Use --ignore-scripts because the root postinstall runs
# `lefthook install`, which needs a .git directory (excluded by
# .dockerignore). lefthook is a dev-only git hook manager and has no
# place inside the Docker build. SKIP_NAPI_BUILD=1 and SKIP_WASM_BUILD=1 tell
# the runtime binding packages to skip native artifact builds. The frontend only
# consumes their TypeScript surfaces, not the runtime binaries.
RUN if [ "$BUILD_FRONTEND" = "true" ]; then \
        export NODE_OPTIONS="--max-old-space-size=8192" && \
        export SKIP_NAPI_BUILD=1 && \
        export SKIP_WASM_BUILD=1 && \
        pnpm install --ignore-scripts && \
        VITE_APP_API_URL="${VITE_APP_API_URL}" VITE_FEATURE_FLAGS="${VITE_FEATURE_FLAGS}" VITE_APP_TURNSTILE_SITE_KEY="${VITE_APP_TURNSTILE_SITE_KEY}" npx turbo build:engine -F @rivetkit/engine-frontend; \
    fi

# Build and copy all binaries from target directory into an empty image (it is not
# included in the output because of cache mount)
RUN \
    --mount=type=secret,id=netrc,target=/root/.netrc,mode=0600 \
    --mount=type=secret,id=DEPOT_TOKEN,env=SCCACHE_WEBDAV_TOKEN \
    --mount=type=cache,target=/usr/local/cargo/git,id=univseral-cargo-git \
    --mount=type=cache,target=/usr/local/cargo/registry,id=univseral-cargo-registry \
    --mount=type=cache,target=/app/target,id=univseral-target \
    --mount=type=cache,target=/root/.cache,id=universal-user-cache \
    set -e && \
    if [ -z "${SCCACHE_WEBDAV_TOKEN:-}" ]; then \
        echo "[sccache] no DEPOT_TOKEN, disabling"; unset RUSTC_WRAPPER; \
    elif ! (sccache --start-server 2>/tmp/sccache-start.err && sccache --show-stats >/dev/null 2>&1); then \
        echo "[sccache] backend health check failed, disabling:"; cat /tmp/sccache-start.err 2>/dev/null || true; \
        sccache --stop-server >/dev/null 2>&1 || true; \
        unset RUSTC_WRAPPER SCCACHE_WEBDAV_ENDPOINT SCCACHE_WEBDAV_TOKEN; \
    else \
        echo "[sccache] enabled via ${SCCACHE_WEBDAV_ENDPOINT}"; \
    fi && \
    if [ "$CARGO_BUILD_MODE" = "release" ]; then \
        RUSTFLAGS="--cfg tokio_unstable" cargo build -p rivet-engine --bin rivet-engine --release; \
    else \
        RUSTFLAGS="--cfg tokio_unstable" cargo build -p rivet-engine --bin rivet-engine; \
    fi && \
    mkdir /app/dist/ && \
    echo "Copying binary" && \
    cp target/$CARGO_BUILD_MODE/rivet-engine /app/dist/ && \
    (sccache --show-stats 2>/dev/null || true)

# MARK: Engine (full, base)
FROM ghcr.io/rivet-dev/rivet/engine-base-runtime-full:a36b881 AS engine-full-base

# MARK: Engine (Full)
FROM engine-full-base AS engine-full

LABEL org.opencontainers.image.source=https://github.com/rivet-dev/rivet

COPY --from=builder /app/dist/rivet-engine /usr/bin/rivet-engine

ENTRYPOINT ["/usr/bin/rivet-engine"]
CMD ["start"]

# MARK: Engine (Slim)
FROM ghcr.io/rivet-dev/rivet/engine-base-runtime-slim:a36b881 AS engine-slim

LABEL org.opencontainers.image.source=https://github.com/rivet-dev/rivet

COPY --from=builder /app/dist/rivet-engine /usr/bin/rivet-engine

ENTRYPOINT ["/usr/bin/rivet-engine"]
CMD ["start"]
