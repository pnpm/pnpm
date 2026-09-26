---
"pacquet": patch
"@pnpm/napi": patch
"@pnpm/deps.status": patch
"pnpm": patch
---

`pnpm install` now returns "Already up to date" in a workspace where `dedupeDirectDeps` left a project without a `node_modules` directory of its own. Such a project forced a full install on every run.
