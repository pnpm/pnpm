---
"@pnpm/os.available-parallelism": minor
"@pnpm/config.reader": patch
"@pnpm/worker": patch
"pnpm": patch
---

`pnpm install` now sizes its worker pool by the CPU quota of the container it runs in, such as a Kubernetes CPU limit, when that quota is smaller than the number of CPUs the container may run on. The default `workspaceConcurrency` respects the quota the same way. On a CI runner with a 4-CPU quota and hundreds of host CPUs, pnpm started hundreds of workers and ran out of memory [#17382](https://github.com/pnpm/pnpm/issues/17382).
