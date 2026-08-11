Use shell commands only. Do not use browser tools, browser skills, repo-wide exploration, or any skill-discovery flow.

Verify the migrated Cloudflare Workflows starter works at `{{URL}}`.

Run exactly these checks:

1. `curl -i {{URL}}`
2. Fetch the original example into a local temp dir with:
   `npx giget gh:cloudflare/workflows-starter#fe87d313936698ba674af56ac9ca3a49704098c6 "$TMPDIR/original-workflows"`
3. Read only:
   - the migrated workflow entrypoint, step definitions, and directly imported HTTP handlers
   - `"$TMPDIR/original-workflows/src/index.ts"`
   - `"$TMPDIR/original-workflows/src/examples.ts"`

Judge criteria:

- The migrated app responds over HTTP.
- Multiple workflow steps are preserved.
- Retry configuration per step is preserved.
- Sleep or delay behavior is preserved.
- Workflow instance creation and status querying remain inspectable.

Return exactly one line of minified JSON:
`{"verdict":"pass"|"fail","reason":"..."}`
