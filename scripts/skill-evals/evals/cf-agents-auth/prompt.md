Fetch the Cloudflare Agents auth example into `./original-template` with:

`/opt/homebrew/opt/node@22/bin/npx giget gh:cloudflare/agents/examples/auth-agent#aba7432d5d395505df88e09b06e1cdd10f5bdad3 ./original-template`

Then migrate it from the Cloudflare Agents SDK to RivetKit. Replace Workers AI with the Vercel AI SDK.

Requirements:
- Keep the original fetched template in `./original-template`
- Write the migrated project at the repository root, not inside `./original-template`
- Use absolute paths for shell tools on this host when needed
- Install dependencies if required
- Finish with a project that runs via `npm run dev` on port 3000
