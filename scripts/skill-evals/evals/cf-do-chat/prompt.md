Fetch the Cloudflare workers-chat-demo into `./original-template` with:

`/opt/homebrew/opt/node@22/bin/npx giget gh:cloudflare/workers-chat-demo#dd32ce87617a9df6c614004d2fc2fb0628698121 ./original-template`

Then migrate it from Cloudflare Durable Objects to RivetKit.

Requirements:
- Keep the original fetched template in `./original-template`
- Write the migrated project at the repository root, not inside `./original-template`
- Use absolute paths for shell tools on this host when needed
- Install dependencies if required
- Finish with a project that runs via `npm run dev` on port 3000
