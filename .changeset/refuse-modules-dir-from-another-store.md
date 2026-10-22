---
"pacquet": patch
---

`pnpm add`, `pnpm remove`, and `pnpm update` now fail if `node_modules` was linked from a different store, a different virtual store directory, or by an incompatible pnpm version. They report `ERR_PNPM_UNEXPECTED_STORE`, `ERR_PNPM_UNEXPECTED_VIRTUAL_STORE`, or `ERR_PNPM_MODULES_BREAKING_CHANGE`. These commands used to remove and reinstall `node_modules` without asking. Only `pnpm install` recreates `node_modules` now.

`pnpm update` now also fails with `ERR_PNPM_HOIST_PATTERN_DIFF` and the other errors `pnpm add` reports for a changed hoist pattern or `virtualStoreDirMaxLength`.
