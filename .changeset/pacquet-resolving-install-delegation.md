---
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

When [`pacquet`](https://github.com/pnpm/pnpm/tree/main/pacquet) (the Rust port of pnpm) is declared in `configDependencies`, pnpm now delegates dependency **resolution** to it too — not just materialization — provided the installed pacquet is new enough to support full resolving installs (>= 0.11.7).

Previously pacquet only ran in frozen-install mode: pnpm always resolved the dependency graph itself (writing `pnpm-lock.yaml`) and handed pacquet a finished lockfile to fetch / import / link. With pacquet >= 0.11.7, a non-frozen `pnpm install` (default isolated `nodeLinker`, plain install) is delegated to pacquet end-to-end in a single pass — pacquet resolves the manifests, writes the lockfile, and materializes `node_modules`. pnpm detects the capability from the installed pacquet's version; older pacquet releases keep the resolve-then-materialize split, and `add` / `update` / `remove` still resolve in pnpm (it has to mutate the manifests first). This remains an opt-in preview of the Rust install engine [#11723](https://github.com/pnpm/pnpm/issues/11723).
