---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm install` now auto-installs missing transitive peers when workspace projects share a dependency at different depths. This also removes incomplete duplicate peer contexts from the lockfile. Fixes [pnpm/pnpm#14840](https://github.com/pnpm/pnpm/issues/14840).
