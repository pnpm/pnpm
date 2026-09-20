---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

`pnpm add <pkg>@<version>` now keeps the specifier a `readPackage` hook provides when the hook rewrites the requested one. The add reports the request as superseded instead of leaving a manifest the next `pnpm install --frozen-lockfile` rejects [#15156](https://github.com/pnpm/pnpm/issues/15156).
