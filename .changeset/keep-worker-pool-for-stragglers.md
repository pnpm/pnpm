---
"@pnpm/worker": patch
"pnpm": patch
---

Fixed `pnpm install` sometimes never exiting after printing `Done in Xs` [#12297](https://github.com/pnpm/pnpm/issues/12297). A worker call arriving after the CLI's final `finishWorkers()` — typically a tarball fetch for a package that is never linked (e.g. a foreign-architecture optional dependency), delayed past install completion by a network retry — lazily re-created the worker pool, and nothing ever finished the new pool, so its idle worker thread kept the process alive until an external timeout killed it. `finishWorkers()` now keeps the finished pool's reference: straggler tasks are still serviced, and their workers are torn down again at check-in.
