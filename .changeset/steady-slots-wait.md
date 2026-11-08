---
"pacquet": patch
---

`pnpm install` now waits for another install that holds a global virtual store slot it needs. An install started by a build script waits at most 10 seconds for such a slot and then fails [pnpm/tasks#284](https://github.com/pnpm/tasks/issues/284).
