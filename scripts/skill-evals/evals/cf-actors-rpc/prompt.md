Fetch the @cloudflare/actors RPC example into `./original-template` with:

`/opt/homebrew/opt/node@22/bin/npx giget gh:cloudflare/actors/examples/rpc#6bbf82b239016ecb205d3b40ff1aa9b8c88b2fa7 ./original-template`

Then migrate it from @cloudflare/actors to RivetKit.

Requirements:
- Keep the original fetched template in `./original-template`
- Write the migrated project at the repository root, not inside `./original-template`
- Use absolute paths for shell tools on this host when needed
- Install dependencies if required
- Finish with a project that runs via `npm run dev` on port 3000
