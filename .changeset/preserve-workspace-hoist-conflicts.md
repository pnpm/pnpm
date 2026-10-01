---
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.hoist": patch
"pnpm": patch
---

Fixed frozen installs replacing a hoisted dependency with a workspace package of the same name. A later `pnpm dedupe` then removed the hoisted link [#16485](https://github.com/pnpm/pnpm/issues/16485).
