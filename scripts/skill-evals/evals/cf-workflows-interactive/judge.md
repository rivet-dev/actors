Verify the migrated Cloudflare Workflows starter template works at {{URL}}.

Clone the original source from https://github.com/cloudflare/templates/tree/30d1642da7e2b42913dc63a4a5ffca9bb01b9679/workflows-starter-template and read all its files. Then read all source files in the migrated project. Use the original as your reference for what features must be present.

## Feature verification

Test every feature present in the original:

1. Open {{URL}} and confirm the React UI loads with workflow controls
2. Start a new workflow and confirm it begins executing
3. Confirm workflow step progress is visible in the UI (steps completing one by one)
4. If the workflow has a `waitForEvent` step (approval gate), confirm the UI shows a waiting state and provides approve/reject controls
5. Approve or reject and confirm the workflow resumes
6. Confirm the workflow completes and final status is shown
7. Check that status updates appear in real-time (via WebSocket)

## Code review

Read through the migrated source and compare against the original. Check that:

- `WorkflowEntrypoint` with `step.do()` is migrated to RivetKit `workflow()` with `ctx.step()`
- `step.sleep()` is migrated to equivalent sleep/delay
- `step.waitForEvent()` is migrated to an equivalent pause mechanism (queue receive, action handler, etc.)
- The Durable Object for real-time status tracking via WebSocket is migrated to actor WebSocket connections
- DO KV storage for step status persistence is migrated to actor state or KV
- The REST API for creating/querying workflow instances is preserved
- The React frontend is migrated to use RivetKit client
- No original functionality was silently dropped

## Pass criteria

- React UI loads without errors
- Can start a workflow
- Workflow steps progress is visible
- Event-based pause and resume works
- Status updates appear in real-time via WebSocket
- Workflow completes successfully
- No original features are missing from the migrated code
