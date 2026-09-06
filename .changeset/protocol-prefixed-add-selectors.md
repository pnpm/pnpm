---
"pacquet": patch
---

`pnpm add` and `pnpm install` now accept protocol-prefixed selectors such as `jsr:@scope/pkg`, `npm:pkg@^1.0.0`, and `workspace:pkg@*`. The package name spelled inside the selector keys the manifest entry. A `jsr:` request is saved with the picked version pinned, so `pnpm add jsr:@scope/pkg` records `jsr:^1.2.3`. These selectors used to fail with `ERR_PNPM_INVALID_DEPENDENCY_NAME` [pnpm/pnpm#14590](https://github.com/pnpm/pnpm/issues/14590).
