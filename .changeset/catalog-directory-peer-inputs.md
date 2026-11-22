---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Frozen installs now detect changes to catalog entries used only by peer dependencies of local directory packages.
