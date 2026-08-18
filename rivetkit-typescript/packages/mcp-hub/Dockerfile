# Build stage
FROM node:22-slim AS builder

WORKDIR /app

# Copy Docker-specific package.json (without workspace dependencies)
COPY rivetkit-typescript/packages/mcp-hub/package.docker.json ./package.json

# Copy Docker-specific configs (without workspace references)
COPY rivetkit-typescript/packages/mcp-hub/tsup.docker.config.ts ./tsup.config.ts
COPY rivetkit-typescript/packages/mcp-hub/tsconfig.docker.json ./tsconfig.json

# Copy source
COPY rivetkit-typescript/packages/mcp-hub/src ./src

# Install all dependencies (including dev for build)
RUN npm install

# Build the package
RUN npm run build

# Production stage
FROM node:22-slim AS runner

WORKDIR /app

# Copy built artifacts
COPY --from=builder /app/dist ./dist
COPY --from=builder /app/package.json ./package.json

# Install production dependencies only
RUN npm install --omit=dev

# Default port
ENV PORT=7332
EXPOSE 7332

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD node -e "fetch('http://localhost:' + (process.env.PORT || 7332) + '/mcp', {method: 'POST'}).then(r => process.exit(r.status < 500 ? 0 : 1)).catch(() => process.exit(1))"

# Run the MCP server
CMD ["node", "dist/cli.js"]
