# Frontend (Ladle) Dockerfile
FROM node:22-alpine AS builder

RUN apk add --no-cache git coreutils

RUN corepack enable && corepack prepare pnpm@latest --activate

WORKDIR /app

# Copy workspace configuration files
COPY pnpm-workspace.yaml package.json pnpm-lock.yaml tsconfig.base.json turbo.json tsup.base.ts ./

# Copy frontend package
COPY frontend/ frontend/

# Copy engine SDK dependencies
COPY engine/sdks/typescript/api-full/ engine/sdks/typescript/api-full/
COPY engine/sdks/typescript/envoy-protocol/ engine/sdks/typescript/envoy-protocol/
COPY rivetkit-typescript/packages/engine-runner/ rivetkit-typescript/packages/engine-runner/
COPY rivetkit-typescript/packages/engine-runner-protocol/ rivetkit-typescript/packages/engine-runner-protocol/

# Copy rivetkit dependencies
COPY rivetkit-typescript/packages/rivetkit/ rivetkit-typescript/packages/rivetkit/
COPY rivetkit-typescript/packages/rivetkit-wasm/ rivetkit-typescript/packages/rivetkit-wasm/
COPY rivetkit-typescript/packages/rivetkit-napi/ rivetkit-typescript/packages/rivetkit-napi/
COPY rivetkit-typescript/packages/engine-cli/ rivetkit-typescript/packages/engine-cli/
COPY rivetkit-typescript/packages/traces/ rivetkit-typescript/packages/traces/
COPY rivetkit-typescript/packages/workflow-engine/ rivetkit-typescript/packages/workflow-engine/

# Copy shared libraries
COPY shared/typescript/virtual-websocket/ shared/typescript/virtual-websocket/

# Copy examples (needed for ladle stories)
COPY examples/ examples/

# Copy generated API docs
COPY rivetkit-asyncapi/ rivetkit-asyncapi/
COPY rivetkit-openapi/ rivetkit-openapi/

# Fetch LFS files
ARG FONTAWESOME_PACKAGE_TOKEN=""
ENV FONTAWESOME_PACKAGE_TOKEN=${FONTAWESOME_PACKAGE_TOKEN}

# Skip the native builds for @rivetkit/rivetkit-wasm and @rivetkit/rivetkit-napi.
# The frontend bundle only needs the committed type declarations (index.d.ts) for
# the rivetkit DTS build to resolve `import("@rivetkit/rivetkit-wasm")` and
# `import("@rivetkit/rivetkit-napi")`; both native builds require a Rust toolchain
# we don't ship in this image.
ENV SKIP_WASM_BUILD=1
ENV SKIP_NAPI_BUILD=1

RUN --mount=type=cache,id=s/465998c9-9dc0-4af4-ac91-b772d7596d6e-pnpm-store,target=/pnpm/store \
    pnpm install --frozen-lockfile

RUN --mount=type=cache,id=s/465998c9-9dc0-4af4-ac91-b772d7596d6e-turbo,target=/app/.turbo \
    npx turbo run build:ladle --filter=@rivetkit/engine-frontend

FROM caddy:alpine

COPY frontend/Caddyfile.ladle /etc/caddy/Caddyfile
COPY --from=builder /app/frontend/build /srv

ENV PORT=80
CMD ["caddy", "run", "--config", "/etc/caddy/Caddyfile"]
