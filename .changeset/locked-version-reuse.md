---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Widening a dependency's range no longer leaves the project on an older version. The lockfile update now points the project at the highest version of that dependency already in the lockfile that satisfies the new range — matching what a full resolution records — instead of keeping the locked version whenever it happened to satisfy, which could leave a duplicate behind. A range change that only an already-locked version satisfies is now also handled without re-resolving [#13778](https://github.com/pnpm/pnpm/issues/13778).
