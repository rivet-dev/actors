---
name: create-launch-post
description: Create and publish concise Rivet launch or changelog posts through an approval-gated workflow, including release research, a required user-selected hero variation, local HTML-rendered social and technical images, an R2 hero upload, MDX authoring, a reviewed draft GitHub PR for manual merge, and a scheduled and verified Buffer thread for @rivet_dev. Use when asked to launch, announce, ship, or prepare and schedule a Rivet feature or release announcement.
---

# Create Launch Post

Prepare a complete, deliberately short launch. Keep the launch date, hero variation, social copy, Buffer schedule, and final GitHub merge as user-owned checkpoints. Opening the draft PR is not completion.

## Workflow contract

Run these phases in order:

1. Accept the user's launch context, research the release, confirm the launch date, and ask the user which hero variation to use. Never silently choose the variation or the painting.
2. Render the 2:1 blog hero and social image. Upload only the blog hero; keep social and technical assets local.
3. Write and validate the blog post, upload the hero, and push an initial draft PR.
4. Give the user one review packet containing the draft PR, social image, first technical-image draft, four Buffer main-post options, and the proposed reply thread. If the feature has meaningful Dashboard, Inspector, or other product UI, recommend adding a clean screenshot and ask the user to provide it or approve capturing it. Iterate on the post, PR, technical image, optional UI screenshot, and social copy until the user approves them. Push every approved blog revision to the same PR.
5. After the user approves the copy, replies, launch date, and exact time, schedule the Buffer thread and reopen it to verify the account, content, image, order, date, time, and timezone. Leave the GitHub PR for the user to merge manually.

Do not declare the launch ready until all three acceptance gates pass:

- **Blog post:** The user has reviewed it, the latest approved revision is pushed to the draft PR, the hero resolves, and relevant checks pass or any external blocker is explicit.
- **Technical image:** The user has reviewed and approved the current local `technical.png`.
- **Buffer:** The approved thread is scheduled for `rivet_dev` and the scheduled item has been verified. If Buffer references a tunneled media URL, a reliable tunnel process and cleanup plan must remain active through publication.

## Parallelize with subagents

Use subagents in bounded waves when concurrency is available. Keep the main agent as the only user-facing orchestrator and the sole owner of tracked-file edits, asset uploads, `jj` operations, GitHub writes, Buffer mutations, and tunnel lifecycle. Give subagents read-only repository work or a unique output directory outside the repository; never let multiple agents edit the same worktree or perform the same external action.

Start wave 1 while the main agent asks for the hero variation and confirms the date:

1. **Release researcher:** Inspect the source PR, implementation, tests, examples, and docs. Return verified public API syntax, constraints, shipped terminology, and candidate documentation and GitHub links with file or URL evidence.
2. **Post-format researcher:** Review at least three recent concise Rivet changelog posts. Return the shared structure, tone, frontmatter conventions, slug rules, and useful examples without editing files.

After the hero variation and release facts are available, start wave 2 in parallel:

1. **Visual worker:** Run the bundled renderer in a unique external output directory. Produce the hero, social image, and first technical-image draft; report paths, dimensions, checksums, and visual concerns. Never upload or commit them.
2. **Blog-draft worker:** Return a concise MDX draft grounded in the verified API and generic launch structure. Do not write the tracked post file.
3. **Distribution worker:** Draft four main Buffer options, the four link replies, and scripted character counts using the required Unicode-code and arrow formatting. Do not schedule anything.

The main agent must reconcile those outputs, verify facts against source, write the MDX, choose what to show the user, and own the approval loop. After integration, use one read-only **QA subagent** to inspect the final diff, post code, URLs, frontmatter/date, rendered images, Buffer payload, and `jj status`. Require it to report concrete defects only; the main agent applies any fixes and reruns focused checks.

Do not delegate user checkpoints or final side effects. The main agent must personally confirm the hero variation, date, copy, thread, exact schedule, PR state, account/channel, media, and final verification.

## 1. Work locally and research the release

1. Read the repository `AGENTS.md` files that govern the files in scope.
2. Work from the local checkout at `~/rivet`; never author repository files through GitHub's contents or object APIs. Reuse a user-provided isolated workspace, or create one before editing:

```bash
jj -R ~/rivet git fetch
jj -R ~/rivet workspace add \
  --name "launch-<slug>" \
  --revision "main@origin" \
  --message "docs(website): launch <feature>" \
  "$WORKSPACE_PATH"
cd "$WORKSPACE_PATH"
```

3. Run `jj status` and keep the launch revision isolated from unrelated work. Put generated and downloaded files outside the repository workspace.
4. Read the source PR, changed docs, examples, tests, and public API. Treat implemented code and merged docs as authoritative; do not infer syntax from the PR title.
5. Read at least three recent concise posts in `website/src/content/posts/`, preferring `category: changelog` posts for similar features.
6. Resolve the canonical post URL, public docs route, GitHub repository, and Discord URL before drafting. The route preserves the complete dated post folder name. If the file is `website/src/content/posts/YYYY-MM-DD-<slug>/page.mdx`, its post ID is `YYYY-MM-DD-<slug>` and its URL is:

```text
category: changelog → https://rivet.dev/changelog/YYYY-MM-DD-<slug>/
all other categories → https://rivet.dev/blog/YYYY-MM-DD-<slug>/
```

Do not remove the date prefix. The Astro routes derive the slug with `entry.id.replace(/\/page$/, "")`, which removes only `/page` and preserves the full folder name.

7. Propose the target launch date and get explicit user confirmation. Use that date consistently for the folder name, `published` frontmatter, asset key, canonical URL, and Buffer schedule.

## 2. Render the launch images

Ask the user which hero variation to use before rendering anything. This is a required creative checkpoint; never choose the variation silently. Both variations write `image.png` at `2048x1024` for the blog hero and `social.png` at `2048x1238` for distribution, so every later phase is identical regardless of which one is chosen. Use the repository renderers; do not use Figma.

### Variation A — painting

The default editorial treatment: a full-bleed painting with the launch title beneath it. Best when the launch is a product or capability announcement with no natural logo lockup.

1. Ask the user to choose and provide a painting. Ask for the source or confirm that it is safe to publish.
2. Pass a short feature name such as `Cron Jobs` as the social title:

```bash
pnpm --dir website render-launch-images -- \
  --painting "$PAINTING_PATH" \
  --title "$SOCIAL_TITLE" \
  --output-dir "$OUTPUT_DIR"
```

3. If the source is already 2:1, preserve its composition and only normalize it to `2048x1024`. Otherwise, visually inspect both PNGs and rerun with `--focal-x <0..1>` and/or `--focal-y <0..1>` if the crop misses the subject. Do not hand-edit the output.

### Variation B — logo tiles

The Rivet wordmark above the launch title, over two to four product logos in rotated rounded "app tiles" with the middle tile dominant. Best when the launch is an integration or a combination of named technologies, where the logo lockup states the story faster than prose. The tile treatment matches the agentOS launch graphics and the landing page's floating-agent tiles.

1. Ask the user which logos belong in the lockup and in what order. Order is left to right, and with three tiles the middle one is the largest, so put the subject of the launch in the middle.
2. Collect an SVG per logo. Reuse repository marks where they exist (`website/src/images/products/`, `website/public/images/registry/`), fall back to the vendored marks in `.claude/skills/create-launch-post/assets/logos/`, and vendor any new mark there. Marks authored white for dark UIs are recolored to the site ink automatically, so a white-on-transparent SVG is fine.
3. Pass the full post title rather than a short feature name; this layout has room for a sentence-length title:

```bash
pnpm --dir website render-tile-images -- \
  --title "$POST_TITLE" \
  --logos "$LOGO_A,$LOGO_B,$LOGO_C" \
  --output-dir "$OUTPUT_DIR"
```

4. Inspect both PNGs. If a title wraps to a third line or a tile crowds the title, shorten the title rather than hand-editing the output. Tile geometry lives in `TILE_LAYOUTS` in the renderer; change it there if a lockup genuinely needs different placement.

When short code demonstrations would help launch distribution, create a temporary JSON file outside the repository with one to four sections:

```json
[
	{
		"title": "Crontab with timezone",
		"language": "ts",
		"code": "await c.cron.set({ /* ... */ });"
	}
]
```

Render the technical image directly without generating hero or social assets:

```bash
pnpm --dir website render-technical-image -- \
  --title "Cron Jobs" \
  --technical-snippets "$TECHNICAL_SNIPPETS_PATH" \
  --output "$OUTPUT_DIR/technical.png"
```

Use `--layout vertical` to stack every snippet at full width; the default `grid` layout uses the launch-image column rules. Pass `--browser /path/to/chromium` only when neither bundled nor common system Chromium installations are available automatically. Normally create the first technical draft immediately after opening the initial blog PR, once the code example is stable enough to summarize. The renderer writes the PNG and adjacent HTML at a fixed `2048px` width with content-driven height, so short examples produce a shorter image. The white technical image places the Perfectly Nineties launch title at top left and the Rivet logo at top right, with matching 132px side and bottom gutters. Section headings use Manrope; code uses four-space tabs plus the website's JetBrains Mono font, Shiki configuration, customized Ayu Dark theme, and code-block geometry. With three grid sections, the first section fills the left column while the other two stack on the right. Every dark code panel stretches to the full available section height so column bottoms align. Keep snippets brief enough to fit, show it to the user, and rerender until approved.

When the feature has useful Dashboard, Inspector, or other visible product UI, proactively recommend a supporting screenshot. Ask the user to provide the screenshot or approve the exact authenticated view and state to capture. Use a clean launch-ready environment, crop away irrelevant browser chrome, and inspect the result for credentials, private names, IDs, tokens, customer data, or other sensitive information. Keep the screenshot local and show it to the user for approval. Confirm whether it should be attached to the first Buffer post or used in a dedicated reply; do not add it automatically.

Do not add `image: true` to the post until the uploaded asset resolves successfully.

## 3. Upload the hero

Use the complete dated post folder name as `<post-id>` and upload the blog hero:

```bash
AWS_ACCESS_KEY_ID='op://Engineering/rivet-assets R2 Upload/username' \
AWS_SECRET_ACCESS_KEY='op://Engineering/rivet-assets R2 Upload/password' \
op run -- aws s3 cp "$IMAGE_PATH" \
  "s3://rivet-assets/website/blog/<post-id>/image.png" \
  --endpoint-url https://2a94c6a0ced8d35ea63cddc86c2681e7.r2.cloudflarestorage.com
```

Verify `https://assets.rivet.dev/website/blog/<post-id>/image.png`, then add `image: true` to frontmatter. Never commit the exported hero to Git and never print resolved credentials.

Return `social.png`, optional `technical.png`, and any approved UI screenshot to the user as local launch-distribution assets. Never upload or commit distribution images, their HTML source, the technical-snippet JSON, the UI screenshot, or the painting source.

## 4. Bootstrap the MDX

Run the repository generator:

```bash
pnpm --dir website new-post
```

For a future launch date, bootstrap normally, then rename the generated folder and update `published` to `YYYY-MM-DD`. The final path is:

```text
website/src/content/posts/YYYY-MM-DD-<slug>/page.mdx
```

Use `author: nathan-flurry` unless the user specifies another author, `category: changelog`, and targeted lowercase keywords.

## 5. Write the generic launch format

Keep the body brief and use this order:

1. One short paragraph explaining what shipped and why it matters.
2. Short capability-focused H2 sections with one small example each, copied from or checked against the release. Give distinct public APIs separate sections when that makes the launch easier to scan; for example: crontab with timezone, fixed intervals, run history, and one-off jobs.
3. Use `## Show Me The Code` only when one cohesive example explains the feature better than separate API sections.
4. `## More Links` with docs, the relevant GitHub repository, and `https://rivet.dev/discord`.

Aim for under 200 prose words excluding code. Include all TypeScript imports and referenced definitions. Avoid roadmap claims, implementation detail, a long feature inventory, or repeated calls to action.

## 6. Validate

1. Confirm the folder date, `published`, title, description, keywords, and image slug agree.
2. Check the code against the source PR and current documentation.
3. Run the narrowest available website formatting/type checks, then `pnpm --dir website build` when dependencies and checkout scope allow it.
4. Review `jj diff` and `jj status`; keep unrelated changes out of the revision.
5. Confirm the blog image URL returns successfully before leaving `image: true` enabled.
6. Derive the canonical URL from the post file instead of rewriting the slug by hand:

```bash
POST_FILE="website/src/content/posts/YYYY-MM-DD-<slug>/page.mdx"
POST_ID="$(basename "$(dirname "$POST_FILE")")"
POST_CATEGORY="$(awk '/^category:/ { print $2; exit }' "$POST_FILE")"
POST_SECTION="blog"
if [ "$POST_CATEGORY" = "changelog" ]; then
  POST_SECTION="changelog"
fi
CANONICAL_URL="https://rivet.dev/$POST_SECTION/$POST_ID/"
printf '%s\n' "$CANONICAL_URL"
```

Cross-check the result against `website/src/pages/changelog/[...slug].astro` or `website/src/pages/blog/[...slug].astro`, and use the exact same URL in the Buffer reply. If the post is deployed, require the exact URL including its trailing slash to return HTTP 200. Also probe any shorter hand-written alternative; a 404 is evidence that it must not be used. For an unmerged future post, verify the source-derived route before scheduling and repeat the HTTP check after deployment but before publication.

## 7. Open a draft PR and continue the review loop

Only push when the user has requested publishing to GitHub. Create an `agent/<description>` bookmark for the revision, push that bookmark, and open a draft PR against `main`. Use a conventional PR title and a short bullet-list body covering the post, artwork, skill changes, and validation.

Return the PR URL and explicitly tell the user that they must review and merge it manually. Continue by drafting the technical image and Buffer options; do not treat PR creation as the end of the task. Apply the user's blog feedback locally, rerun relevant checks, and push the updated revision to the same PR. Never merge the launch PR on the user's behalf.

## 8. Prepare the @rivet_dev Buffer thread

Draft the Buffer options after the initial blog PR and technical image exist. Do not schedule anything until the user approves the blog post, technical image, main copy, reply thread, date, and exact time.

1. Draft four distinct options for the main `@rivet_dev` post. Match the concise launch style: a direct opening line, the core value, and a few specific emoji-led points when useful. Do not put a URL in the main post.
2. Prefer a code-first structure when the API surface is central to the launch. Surface the API bullets before supporting prose or secondary benefits so readers see the shipped interface immediately.
3. Render short code identifiers with Unicode Mathematical Monospace characters instead of backticks. For example, render `c.cron.set` as `𝚌.𝚌𝚛𝚘𝚗.𝚜𝚎𝚝` and `c.schedule.after` as `𝚌.𝚜𝚌𝚑𝚎𝚍𝚞𝚕𝚎.𝚊𝚏𝚝𝚎𝚛`. Apply the treatment only to code fragments, preserve punctuation such as periods, and keep normal prose unstyled. Verify that Buffer renders every identifier legibly; fall back to ASCII if it does not.
4. Format API bullets as `<emoji> <code identifier> → <short benefit>`. Use the right arrow, not an em dash, and keep the benefit lowercase unless it contains a proper noun.
5. Keep every option strictly below X's 280-character limit. Target at most 250 Unicode code points and show the count beside each option. Before presenting any option, run this sanity check with the complete candidate text, including newlines and emoji; do not present or schedule text when the script exits nonzero:

```bash
LAUNCH_POST_TEXT="$CANDIDATE_TEXT" node - <<'NODE'
const text = process.env.LAUNCH_POST_TEXT ?? "";
const count = Array.from(text).length;
console.log(`${count} Unicode code points`);
if (count >= 280) process.exit(1);
NODE
```

Treat Buffer's composer counter as authoritative before scheduling because platform weighting can differ from the sanity check. Shorten any option Buffer reports as 280 or more.

6. Draft these as four separate follow-up replies, using resolved URLs rather than placeholders:

```text
Read more: <canonical rivet.dev post URL>
GitHub: <https://github.com/rivet-dev/rivet or https://github.com/rivet-dev/agentos>
Documentation: <relevant public docs URL>
Discord: https://rivet.dev/discord
```

7. Ask the user to choose one main-post option or provide their own, edit or approve the four replies, and confirm the launch date. Once the user selects or supplies the copy, treat that version as canonical: stop proposing alternatives and change only the parts they request.
8. Select a random whole-minute time from 7:30 through 8:30 AM in `America/Los_Angeles`, show the exact date, time, and timezone, and get explicit confirmation. The date must match the post's `published` date.

After approval, prefer the Executor `buffer_mcp` integration and fall back to the authenticated Buffer web app only when the MCP lacks a required capability:

1. Load the Executor `execute` guidance, search the `buffer_mcp` catalog, and use the exact connected tool paths returned by search. Do not guess paths or call Buffer's raw API.
2. Call `get_account` first. If it returns multiple organizations, list them and get the user's choice; otherwise use the sole organization. Call `list_channels`, select the exact `rivet_dev` Twitter channel, then call `get_channel` and verify it is connected, unlocked, and configured for `America/Los_Angeles`.
3. Call `list_posts` for that channel and launch date with `scheduled` and `sending` statuses. Stop if the same launch is already present instead of creating a duplicate.
4. Call `create_post` with `schedulingType: "automatic"`, `mode: "customScheduled"`, and an ISO-8601 `dueAt` carrying the Pacific UTC offset. Put the approved main copy in both the outer `text` field and the first `metadata.twitter.thread` item; include all replies in that thread array. Put the approved `social.png` asset in both the outer `assets` array and the first `metadata.twitter.thread[0].assets` array, with no link in its text. Buffer may drop threaded media if it is supplied in only one location. Do not use `technical.png` instead or attach an optional dashboard screenshot unless the user explicitly approves that media and placement.
5. Add the approved Read more, GitHub, Documentation, and Discord replies in that order. Do not collapse them into the main post.
6. Call `get_post` and verify the post ID, `status: scheduled`, exact `rivet_dev` channel ID, complete text and reply order, due time, image source, dimensions and alt text, and `error: null`. Confirm the returned UTC time converts to the approved Pacific time.
7. When changing a scheduled post, call `get_post` first and require `updatePost` in `allowedActions`. Buffer's `edit_post` is a whole-post replacement, not a patch: carry forward the scheduling mode, due time, all replies, metadata, and approved assets, then change only what the user requested. For a Twitter thread, include the existing image in both outer `assets` and `metadata.twitter.thread[0].assets`; immediately call `get_post` and repair the post before continuing if its returned `assets` array is empty. Run the character sanity check again and call `get_post` after editing.
8. Return the Buffer post ID or URL and the verified schedule. Treat schedule-and-verify as the required Buffer publication gate.

Prefer a direct media upload when the Buffer integration provides one. If the Executor Buffer MCP only accepts an image URL and has no upload tool, use this temporary single-file handoff instead of uploading the distribution image to Git or Rivet assets:

1. Create a temporary directory outside the repository containing only the approved image, and serve it from a loopback-only HTTP server on an unused high port. Never expose the repository or the full output directory.
2. Open a temporary public tunnel to that loopback server. Before scheduling, download the tunneled URL and confirm its SHA-256 checksum and dimensions match the local image.
3. Pass the temporary image URL to Buffer with dimensions and descriptive alt text. Buffer retains this source URL; a successful `HEAD` or `GET` during creation does not prove that Buffer copied the image.
4. Reopen the scheduled post through Buffer and verify `status: scheduled`, the channel, complete thread, due time, image URL, type and dimensions, and alt text. Load the image URL independently to confirm it still returns the approved file.
5. Keep the loopback server and tunnel reachable until the post has published. After Buffer reports `status: sent` and the published post renders the image, stop both processes and delete the temporary directory. If reliable tunnel uptime through publication is not possible, do not use a quick tunnel; use Buffer's authenticated direct-upload UI or a user-approved stable host instead.

Never commit the distribution image or tunnel URL, and never upload it to Rivet assets without explicit user approval.

If Buffer access, the verified company account, or threaded scheduling is unavailable, return the approved copy and exact schedule as a handoff. Never claim that the thread was scheduled.

If the hero variation, painting, logo marks, asset credentials, or release API is missing, complete the safe independent work and report the exact remaining checkpoint instead of fabricating it.
