Fetch the Cloudflare queues-web-crawler into `./original-template` with:

`/opt/homebrew/opt/node@22/bin/npx giget gh:cloudflare/queues-web-crawler#7d9bd009881e26e852ae850d739a70837df0dd67 ./original-template`

Then migrate it from Cloudflare Queues to RivetKit. Replace Cloudflare Browser Rendering with Browserbase, Browserless, or Playwright.

Requirements:
- Keep the original fetched template in `./original-template`
- Write the migrated project at the repository root, not inside `./original-template`
- Use absolute paths for shell tools on this host when needed
- Install dependencies if required
- Finish with a project that runs via `npm run dev` on port 3000
