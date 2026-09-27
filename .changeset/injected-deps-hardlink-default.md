---
"pacquet": patch
---

An in-place edit to the source of an injected workspace package now shows up in its injected copy, unless a build writes to that package or `packageImportMethod` is set. pnpm hardlinks such packages under the default import method [pnpm/pnpm#4410](https://github.com/pnpm/pnpm/issues/4410).
