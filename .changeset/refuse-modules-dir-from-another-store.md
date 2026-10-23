---
"pacquet": patch
---

`pnpm add`, `pnpm remove`, and `pnpm update` now fail if `node_modules` was linked from a different store, a different virtual store directory, or by an incompatible pnpm version. They report `ERR_PNPM_UNEXPECTED_STORE`, `ERR_PNPM_UNEXPECTED_VIRTUAL_STORE`, or `ERR_PNPM_MODULES_BREAKING_CHANGE`. They also fail with `ERR_PNPM_NODE_LINKER_DIFF` if `node_modules` was installed with a different `nodeLinker`. Previously, these commands removed and reinstalled `node_modules` without asking. Run `pnpm install` to recreate it.

`pnpm install` now recreates `node_modules` after a `storeDir` or `virtualStoreDir` change. It used to report "Already up to date".

`pnpm update` now also fails with `ERR_PNPM_HOIST_PATTERN_DIFF` and the other errors `pnpm add` reports for a changed hoist pattern or `virtualStoreDirMaxLength`.
