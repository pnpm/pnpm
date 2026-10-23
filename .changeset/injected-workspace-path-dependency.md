---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

A `workspace:` dependency written as a relative path, such as `workspace:../lib-a`, in an injected workspace package now resolves from that package's directory [#17284](https://github.com/pnpm/pnpm/issues/17284).
