---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm add` now keeps the specifier a `readPackage` hook provides when the hook rewrites the requested one. When the hook removes the dependency instead, the add skips it and reports why. Previously the add wrote the request either way, and the hook undid it on the next read, so `pnpm install --frozen-lockfile` failed [#15156](https://github.com/pnpm/pnpm/issues/15156).
