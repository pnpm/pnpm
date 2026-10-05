---
"@pnpm/deps.compliance.license-scanner": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.list": patch
"@pnpm/deps.inspection.tree-builder": patch
"@pnpm/deps.path": patch
"pacquet": patch
"pnpm": patch
---

`pnpm list` now reports the correct package paths when `nodeLinker` is `hoisted` [#9593](https://github.com/pnpm/pnpm/issues/9593).
