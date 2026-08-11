Use shell commands only. Do not use browser tools, browser skills, repo-wide exploration, or any skill-discovery flow.

Verify the migrated Durable Objects hello world template works at `{{URL}}`.

From the current working directory, start the app yourself if it is not already running. Prefer the declared project script instead of inventing a different startup path. If startup fails, inspect the startup output and explain the concrete root cause in the verdict. Do not inspect or modify sibling `skill-eval-*` directories. Treat the project as read-only: do not edit any project files or use shell commands that mutate them.

Run exactly these checks:

1. `/usr/bin/curl -fsS {{URL}}`
2. `/usr/bin/curl -fsS {{URL}}` again to confirm the response is stable
3. Fetch the original example into a local temp dir with:
   `/opt/homebrew/opt/node@22/bin/npx giget gh:cloudflare/templates/hello-world-do-template#30d1642da7e2b42913dc63a4a5ffca9bb01b9679 "$TMPDIR/original-do"`
4. Read only:
   - the migrated main entrypoint and any directly imported local files needed to understand it
   - `"$TMPDIR/original-do/src/index.ts"`

Judge criteria:

- The HTTP response contains `Hello, World!` or an equivalent greeting.
- The migrated code routes HTTP requests into a RivetKit actor.
- The actor exposes an action equivalent to `sayHello`.
- Actor identity is derived from a stable key.
- The migrated code uses actor SQLite. If it does not, fail.

Return exactly one line of minified JSON:
`{"verdict":"pass"|"fail","reason":"..."}`
