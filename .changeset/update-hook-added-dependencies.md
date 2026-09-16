---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm audit --fix=update` no longer writes hook-injected dependencies to `package.json` or rewrites their hook-provided lockfile specifiers [#14928](https://github.com/pnpm/pnpm/issues/14928).
