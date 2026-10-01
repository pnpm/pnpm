---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

A repeat `pnpm install` reports "Already up to date" again when an optional dependency failed to build. Before, every install reran the failing build, which made a no-op install take minutes on Windows [#16468](https://github.com/pnpm/pnpm/issues/16468).
