Fetch the Cloudflare Durable Objects hello world example into `./original-template` with:

`/opt/homebrew/opt/node@22/bin/npx giget gh:cloudflare/templates/hello-world-do-template#30d1642da7e2b42913dc63a4a5ffca9bb01b9679 ./original-template`

Then migrate it from Cloudflare Durable Objects to RivetKit.

Requirements:
- Keep the original fetched template in `./original-template`
- Write the migrated project at the repository root, not inside `./original-template`
- Use absolute paths for shell tools on this host when needed
- Install dependencies if required
- Finish with a project that runs via `npm run dev` on port 3000
