# Dependency management

## pnpm workspace

- Use pnpm for all npm-related commands. This is a pnpm workspace.

## RivetKit package resolutions

- The root `/package.json` contains `resolutions` that map RivetKit packages to local workspace versions (`"rivetkit": "workspace:*"`, `"@rivetkit/react": "workspace:*"`, etc.).
- Add new internal `@rivetkit/*` packages to root `resolutions` with `"workspace:*"` if missing.
- Prefer re-exporting internal packages (for example `@rivetkit/workflow-engine`) from `rivetkit` subpaths like `rivetkit/workflow` instead of direct dependencies.
- In `/examples/` dependencies, use `*` as the version because root resolutions map them to local workspace packages.

## Rust workspace deps

- When adding a Rust dependency, check for a workspace dependency in `Cargo.toml` first.
- If available, use the workspace dependency (e.g., `anyhow.workspace = true`).
- If missing, add it to `[workspace.dependencies]` in root `Cargo.toml`, then reference it with `{dependency}.workspace = true` in the consuming package.

## Dynamic imports for runtime-only deps

- For runtime-only dependencies, use dynamic loading so bundlers do not eagerly include them.
- Build the module specifier from string parts (for example with `["pkg", "name"].join("-")` or `["@scope", "pkg"].join("/")`) instead of a single string literal.
- Prefer this pattern for modules like `@rivetkit/rivetkit-napi/wrapper`, `sandboxed-node`, and `isolated-vm`.
- The TypeScript registry's native envoy path dynamically loads `@rivetkit/rivetkit-napi` and `@rivetkit/engine-cli` so browser and serverless bundles do not eagerly pull native-only modules.
- If loading by resolved file path, resolve first and then import via `pathToFileURL(...).href`.

## Version bumps

- When adding or changing any version value in the repo, verify `scripts/publish/src/lib/version.ts` (`bumpPackageJsons` for package.json files, `updateSourceFiles` for Cargo.toml + examples) updates that location so release bumps cannot leave stale versions behind.

## reqwest clients

- Engine code must use `rivet_pools::reqwest::client().await?` instead of creating a new reqwest client.
- Standalone CLIs may create their own reqwest clients without depending on Engine pools. Preserve the workspace TLS root configuration and configure timeouts, redirects, and credential-safe errors explicitly.
