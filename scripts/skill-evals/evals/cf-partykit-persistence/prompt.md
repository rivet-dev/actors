Fetch the PartyKit persistence example into `./original-template` with:

`/opt/homebrew/opt/node@22/bin/npx giget gh:partykit/partykit/examples/persistence#5527a744d25ff051a204806b85af504cb0fe2f7b ./original-template`

Then migrate it from PartyKit to RivetKit.

Requirements:
- Keep the original fetched template in `./original-template`
- Write the migrated project at the repository root, not inside `./original-template`
- Use absolute paths for shell tools on this host when needed
- Install dependencies if required
- Finish with a project that runs via `npm run dev` on port 3000
