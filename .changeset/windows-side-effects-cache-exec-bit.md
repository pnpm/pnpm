---
"pacquet": patch
---

On Windows, `pnpm install` no longer skips a dependency's build script on a later install when the package ships an executable file and the script changes nothing inside the package directory [#15667](https://github.com/pnpm/pnpm/issues/15667).
