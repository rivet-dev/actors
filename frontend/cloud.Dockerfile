# Frontend (Cloud) Dockerfile
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

# Copy examples
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

RUN --mount=type=cache,id=s/47975eb7-74fd-4043-a505-62b995ff5718-pnpm-store,target=/pnpm/store \
    pnpm install --frozen-lockfile

ARG VITE_APP_API_URL="https://VITE_APP_API_URL.placeholder.rivet.dev"
ARG VITE_APP_CLOUD_API_URL="https://VITE_APP_CLOUD_API_URL.placeholder.rivet.dev"
ARG VITE_APP_ASSETS_URL="https://VITE_APP_ASSETS_URL.placeholder.rivet.dev"
ARG VITE_APP_SENTRY_DSN="https://VITE_APP_SENTRY_DSN.placeholder.rivet.dev/0"
ARG VITE_APP_SENTRY_PROJECT_ID="0"
ARG VITE_APP_POSTHOG_API_KEY=""
ARG VITE_APP_POSTHOG_HOST=""
ARG VITE_FEATURE_FLAGS=""
ARG VITE_APP_TURNSTILE_SITE_KEY=""
ARG DEPLOYMENT_TYPE="staging"

ENV VITE_APP_API_URL=${VITE_APP_API_URL}
ENV VITE_APP_CLOUD_API_URL=${VITE_APP_CLOUD_API_URL}
ENV VITE_APP_ASSETS_URL=${VITE_APP_ASSETS_URL}
ENV VITE_APP_SENTRY_DSN=${VITE_APP_SENTRY_DSN}
ENV VITE_APP_SENTRY_PROJECT_ID=${VITE_APP_SENTRY_PROJECT_ID}
ENV VITE_APP_POSTHOG_API_KEY=${VITE_APP_POSTHOG_API_KEY}
ENV VITE_APP_POSTHOG_HOST=${VITE_APP_POSTHOG_HOST}
ENV VITE_FEATURE_FLAGS=${VITE_FEATURE_FLAGS}
ENV VITE_APP_TURNSTILE_SITE_KEY=${VITE_APP_TURNSTILE_SITE_KEY}
ENV VITE_APP_SENTRY_ENV=${DEPLOYMENT_TYPE}
ENV VITE_DEPLOYMENT_TYPE=${DEPLOYMENT_TYPE}
ENV FONTAWESOME_PACKAGE_TOKEN=${FONTAWESOME_PACKAGE_TOKEN}
ENV VITE_APP_SENTRY_TUNNEL="/tunnel"

RUN --mount=type=cache,id=s/47975eb7-74fd-4043-a505-62b995ff5718-turbo,target=/app/.turbo \
    npx turbo run build --filter=@rivetkit/engine-frontend

FROM caddy:alpine

RUN apk add --no-cache bash

COPY frontend/Caddyfile /etc/caddy/Caddyfile
COPY --from=builder /app/frontend/dist /srv
COPY frontend/docker-entrypoint.sh /docker-entrypoint.sh
RUN chmod +x /docker-entrypoint.sh

ENV PORT=80
ENTRYPOINT ["/docker-entrypoint.sh"]
CMD ["caddy", "run", "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile"]
