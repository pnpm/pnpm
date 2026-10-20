---
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` when an injected workspace package depends on a local directory under another name, such as `"renamed": "file:../b"` [#17130](https://github.com/pnpm/pnpm/issues/17130).
