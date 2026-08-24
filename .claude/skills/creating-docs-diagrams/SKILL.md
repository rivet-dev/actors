---
name: creating-docs-diagrams
description: "Creates accessible hand-authored inline SVG architecture and sequence diagrams for documentation. Use when adding, replacing, or reviewing docs diagrams, especially Mermaid flowcharts."
---

# Creating Docs Diagrams

Create literal, responsive raw SVG that fits Rivet's self-hosted perimeter aesthetic.

## Scope

- Apply raw SVG conversion only to documentation pages under `website/src/content/docs/**/*.mdx`.
- Keep every diagram under `website/src/content/cookbook/**` as Mermaid. Do not convert cookbook Mermaid to raw SVG, including during a repository-wide diagram cleanup.
- Treat posts, changelog entries, and other content trees as out of scope unless the user explicitly names them.

## Workflow

1. Inventory Mermaid before editing with `rg -n '```mermaid' website/src/content/docs` and record every node, boundary, edge direction, edge label, and line break. Do not broaden this search into cookbooks or posts for conversion work.
2. Choose an **architecture pattern** for topology and nested deployment boundaries, or a **sequence pattern** for time-ordered messages between participants.
3. Replace each block with literal inline `<svg>...</svg>`. Do not use Mermaid, scripts, imported components, external SVGs, or image assets.
4. Preserve every semantic detail. Keep `<-->` bidirectional with markers at both ends, `-->` directional, nested boundaries intact, and explicit `<text>` lines for labels that used `<br/>`.
5. Give every SVG `role="img"` and a concise but complete `aria-label`. Give marker IDs unique, descriptive names per page and SVG.
6. Start the website preview and screenshot every changed diagram at a desktop viewport and at 390 CSS pixels. In Playwright, pass `viewport`, not `viewportSize`, and assert `window.innerWidth` before trusting a mobile capture. Activate every tab so hidden diagrams are also captured.
7. Keep sticky site chrome out of element screenshots by scrolling the target below it or hiding the chrome only for the capture. For a horizontally scrollable diagram, capture the full diagram at desktop plus its left and right scroll extents at 390px; record `clientWidth` and `scrollWidth` to confirm the screenshots exercise real overflow.
8. Inspect screenshot pixels, not only DOM bounds. Reject excessive dead space, unnecessarily long arrows, text touching a line, paths crossing unrelated nodes, clipped labels, or inconsistent node styling. Fix and recapture until every diagram passes.
9. Run `rg -n '```mermaid|foreignObject|<style' <changed-pages>` as appropriate, then run Astro or the website build validation.

## Visual contract

- Derive the canvas width from its content. Do not default every diagram to 760px. As a starting range, use 480-560px for two participants, 600-700px for three, 700-800px for four, and only exceed 800px when labels demonstrably require it.
- Keep participants close enough that arrows communicate a relationship instead of becoming long horizontal rules. A two-participant sequence must not place its lifelines at opposite edges of a 760px canvas.
- Outer SVG: `style="width:100%;max-width:<canvas-width>px;height:auto;display:block;margin:2.5rem auto;font-family:system-ui,sans-serif"` with a `viewBox` matching that natural width.
- Simple diagrams may scale fluidly when labels remain readable. Detail-dense diagrams may use `<div style="overflow-x:auto"><div style="min-width:<canvas-width>px">...</div></div>`, but the minimum width must equal the diagram's compact natural width, not a shared global width.
- Deployment and machine boundaries: dashed pine `#2E4034`, `#faf8f3` fill, rounded corners, and uppercase mono labels.
- Nodes: `#ffffff`, warm-black `#1b1916` 1.3 to 1.4px hairlines, and `rx="7"`. Highlight the key runtime or storage node with pale pine `#e7ece7`.
- Text: warm-black primary labels and `#56524a` or `#8a8578` secondary labels. Use `system-ui,sans-serif`; use `ui-monospace,monospace` for boundary labels.
- Arrows: warm-black or pine, 1.4px. Route paths around nodes and minimize crossings. Place an opaque porcelain mask behind any label that intersects an arrow or lifeline.
- At 390px, preserve readable labels without forcing users to scroll through avoidable empty space. Horizontal scrolling is a fallback for real density, not a substitute for laying out a compact diagram.
- Avoid bright Mermaid colors, CSS classes, `foreignObject`, gradients, shadows, JavaScript, and external assets.

## Compact patterns

Architecture topology:

```svg
<svg viewBox="0 0 760 180" role="img" aria-label="Service connects bidirectionally to the runtime inside your infrastructure." style="width:100%;max-width:760px;height:auto;display:block;margin:2.5rem auto;font-family:system-ui,sans-serif">
  <defs><marker id="page-architecture-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" fill="#2E4034"/></marker></defs>
  <rect x="20" y="20" width="720" height="140" rx="10" fill="#faf8f3" stroke="#2E4034" stroke-width="1.4" stroke-dasharray="7 6"/>
  <text x="40" y="44" font-size="11" font-family="ui-monospace,monospace" letter-spacing="0.14em" fill="#2E4034">YOUR INFRASTRUCTURE</text>
  <rect x="100" y="70" width="180" height="56" rx="7" fill="#ffffff" stroke="#1b1916" stroke-width="1.4"/>
  <rect x="480" y="70" width="180" height="56" rx="7" fill="#e7ece7" stroke="#1b1916" stroke-width="1.4"/>
  <line x1="281" y1="98" x2="479" y2="98" stroke="#2E4034" stroke-width="1.4" marker-start="url(#page-architecture-arrow)" marker-end="url(#page-architecture-arrow)"/>
</svg>
```

Sequence diagrams use participant boxes across the top, dashed vertical lifelines, and horizontal directional message arrows in chronological order from top to bottom. Put each message label immediately above its arrow, for example:

```svg
<div style="overflow-x:auto">
<div style="min-width:560px">
<svg viewBox="0 0 560 240" role="img" aria-label="Client sends a request to the runtime, which returns a response." style="width:100%;max-width:560px;height:auto;display:block;margin:2.5rem auto;font-family:system-ui,sans-serif">
  <defs>
    <marker id="page-sequence-request-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" fill="#1b1916"/></marker>
    <marker id="page-sequence-response-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 L10 5 L0 10 z" fill="#2E4034"/></marker>
  </defs>
  <g fill="#ffffff" stroke="#1b1916" stroke-width="1.4"><rect x="30" y="20" width="150" height="48" rx="7"/><rect x="380" y="20" width="150" height="48" rx="7"/></g>
  <g stroke="#8a8578" stroke-width="1.3" stroke-dasharray="5 5"><line x1="105" y1="68" x2="105" y2="220"/><line x1="455" y1="68" x2="455" y2="220"/></g>
  <text x="280" y="105" text-anchor="middle" font-size="12" fill="#56524a">request</text><line x1="105" y1="116" x2="454" y2="116" stroke="#1b1916" stroke-width="1.4" marker-end="url(#page-sequence-request-arrow)"/>
  <text x="280" y="165" text-anchor="middle" font-size="12" fill="#56524a">response</text><line x1="455" y1="176" x2="106" y2="176" stroke="#2E4034" stroke-width="1.4" stroke-dasharray="5 4" marker-end="url(#page-sequence-response-arrow)"/>
</svg>
</div>
</div>
```
