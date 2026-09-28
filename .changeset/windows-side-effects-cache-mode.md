---
"@pnpm/worker": patch
"pnpm": patch
---

On Windows, `pnpm install` no longer skips a dependency's build script on a later install when the script changes nothing inside the package directory [#15667](https://github.com/pnpm/pnpm/issues/15667).
