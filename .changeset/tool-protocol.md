---
"pacquet": minor
---

Added the `tool:` protocol for the tools pnpm downloads itself: Node.js, Bun, Deno, and Yarn. `pnpm add bun@tool:1.3.0`, `pnx node@tool:22`, and `pnx yarn@tool:4` work like the `runtime:` spelling, which stays supported as an alias. A lockfile written for `runtime:` stays up to date when `package.json` switches to `tool:`, and the other way around.
