---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm add <pkg>@<version>` now keeps the specifier a `readPackage` hook provides when the hook rewrites the requested one. Previously the add wrote the requested specifier, which the hook rewrote away on the next read, so `pnpm install --frozen-lockfile` failed. The add now reports the request as superseded [#15156](https://github.com/pnpm/pnpm/issues/15156).
