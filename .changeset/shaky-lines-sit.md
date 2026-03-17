---
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.tree-builder": patch
"@pnpm/types": patch
"@pnpm/deps.inspection.list": patch
"pnpm": patch
---

`pnpm list` and `pnpm why` now display npm: protocol for aliased packages (e.g., `foo npm:is-odd@3.0.1`) [#8660](https://github.com/pnpm/pnpm/issues/8660).
