Fetch the Cloudflare Workflows starter into `./original-template` with:

`/opt/homebrew/opt/node@22/bin/npx giget gh:cloudflare/workflows-starter#fe87d313936698ba674af56ac9ca3a49704098c6 ./original-template`

Then migrate it from Cloudflare Workflows to RivetKit.

Requirements:
- Keep the original fetched template in `./original-template`
- Write the migrated project at the repository root, not inside `./original-template`
- Use absolute paths for shell tools on this host when needed
- Install dependencies if required
- Finish with a project that runs via `npm run dev` on port 3000
