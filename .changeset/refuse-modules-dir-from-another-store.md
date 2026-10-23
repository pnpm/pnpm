---
"pacquet": patch
---

`pnpm add`, `pnpm remove`, and `pnpm update` now fail if `node_modules` does not match the current settings. Run `pnpm install` to recreate it. Previously, these commands removed and reinstalled `node_modules` without asking. They report:

- `ERR_PNPM_UNEXPECTED_STORE` if `node_modules` was linked from a different store.
- `ERR_PNPM_UNEXPECTED_VIRTUAL_STORE` if it was linked from a different virtual store directory.
- `ERR_PNPM_MODULES_BREAKING_CHANGE` if it was installed by an incompatible pnpm version.
- `ERR_PNPM_NODE_LINKER_DIFF` if it was installed with a different `nodeLinker`.

`pnpm install` now recreates `node_modules` after a `storeDir` or `virtualStoreDir` change. It used to report "Already up to date".

`pnpm update` now also fails with `ERR_PNPM_HOIST_PATTERN_DIFF` and the other errors `pnpm add` reports for a changed hoist pattern or `virtualStoreDirMaxLength`.
