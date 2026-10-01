---
"@pnpm/building.during-install": patch
"pnpm": patch
"pacquet": patch
---

When an optional dependency fails to build, pnpm now removes its link from `node_modules`. A repeat `pnpm install` then reports "Already up to date" and no longer reruns the failing build [#16468](https://github.com/pnpm/pnpm/issues/16468).
