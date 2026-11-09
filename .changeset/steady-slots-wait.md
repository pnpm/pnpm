---
"pacquet": patch
---

`pnpm install` no longer links a package from the global virtual store while another install is still importing or building it. It waits for that install to finish. An install started by a build script waits at most 10 seconds and then fails [pnpm/tasks#284](https://github.com/pnpm/tasks/issues/284).
