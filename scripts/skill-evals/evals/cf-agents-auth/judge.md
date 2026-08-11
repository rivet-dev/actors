Use shell commands only. Do not use browser tools, browser skills, repo-wide exploration, or any skill-discovery flow.

Verify the migrated Cloudflare Agents auth example works at `{{URL}}`.

Run exactly these checks:

1. `curl -i {{URL}}`
2. Fetch the original example into a local temp dir with:
   `npx giget gh:cloudflare/agents/examples/auth-agent#aba7432d5d395505df88e09b06e1cdd10f5bdad3 "$TMPDIR/original-auth-agent"`
3. Read only the migrated auth/server entrypoints plus directly imported local auth files, and compare them against:
   - `"$TMPDIR/original-auth-agent/src/server.ts"`
   - `"$TMPDIR/original-auth-agent/src/auth-client.ts"`
   - `"$TMPDIR/original-auth-agent/src/client.tsx"`

Judge criteria:

- The app responds over HTTP without crashing.
- JWT issuance and verification logic is preserved.
- WebSocket or realtime auth uses a token-based authenticated path.
- HTTP auth rejection for unauthenticated access is implemented.
- The frontend still contains a login/auth flow.
- Workers AI may be replaced, but auth must remain enforced end to end.

Return exactly one line of minified JSON:
`{"verdict":"pass"|"fail","reason":"..."}`
