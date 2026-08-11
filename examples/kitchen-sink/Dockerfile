# Base image is overridable so local builds can match the host glibc. The cloud
# build keeps the node:22-slim default and supplies a glibc-compatible napi
# binary; a modern dev host (newer glibc) should override this with a newer base
# such as node:22-trixie-slim so the host-built napi binary loads.
ARG NODE_IMAGE=node:22-slim
FROM ${NODE_IMAGE}
RUN corepack enable && corepack prepare pnpm@10.13.1 --activate
WORKDIR /app
ENV NODE_OPTIONS=--max-old-space-size=7168
ENV SKIP_NAPI_BUILD=1
ENV SKIP_WASM_BUILD=1

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml turbo.json tsconfig.base.json tsup.base.ts ./
COPY patches/ ./patches/
COPY examples/kitchen-sink/ ./examples/kitchen-sink/
COPY rivetkit-typescript/packages/ ./rivetkit-typescript/packages/
COPY engine/sdks/typescript/ ./engine/sdks/typescript/
COPY shared/typescript/ ./shared/typescript/

RUN pnpm install --frozen-lockfile
RUN mkdir -p rivetkit-openapi rivetkit-asyncapi
RUN pnpm build --filter=kitchen-sink

WORKDIR /app/examples/kitchen-sink

EXPOSE 8080
# Run the bundled server directly so node is pid1. Signals reach the rivetkit
# SIGTERM handler without going through pnpm/tsx wrappers.
CMD ["node", "--enable-source-maps", "dist-server/server.mjs"]
