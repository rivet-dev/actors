Use shell commands only. Do not use browser tools, browser skills, repo-wide exploration, or any skill-discovery flow.

Verify the migrated Cloudflare D1 template works at `{{URL}}`.

Run exactly these checks:

1. `curl -fsS {{URL}}`
2. `curl -fsS {{URL}}` again to confirm the response is stable
3. Fetch the original example into a local temp dir with:
   `npx giget gh:cloudflare/templates/d1-template#30d1642da7e2b42913dc63a4a5ffca9bb01b9679 "$TMPDIR/original-d1"`
4. Read only:
   - the migrated server entrypoint and directly imported schema/query files
   - `"$TMPDIR/original-d1/src/index.ts"`
   - `"$TMPDIR/original-d1/src/renderHtml.ts"`
   - `"$TMPDIR/original-d1/migrations/0001_create_comments_table.sql"`

Judge criteria:

- The HTTP response contains comments data.
- The schema and seed data are preserved.
- D1 query logic is migrated to actor SQLite.
- Schema initialization or migration exists in migrated code.

Return exactly one line of minified JSON:
`{"verdict":"pass"|"fail","reason":"..."}`
