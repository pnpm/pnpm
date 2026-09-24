---
"pacquet": patch
---

`pnpm install` links packages from the store faster and with less CPU. The files of one directory are now linked one after another by a single worker, while different directories still link in parallel. Before, every file was its own task, and workers linking into the same directory queued on the directory lock. On Windows this made a warm install several times slower than pnpm 11 [#15439](https://github.com/pnpm/pnpm/issues/15439).
