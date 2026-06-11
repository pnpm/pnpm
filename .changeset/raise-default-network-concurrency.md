---
"@pnpm/installing.package-requester": minor
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

Raised the default network concurrency from `min(64, max(cpuCores * 3, 16))` to `min(96, max(cpuCores * 3, 64))`. Package downloads are I/O-bound, not CPU-bound, so deriving the floor from the core count left machines with few cores (for example 4-vCPU CI runners) downloading only 16 tarballs at a time and unable to saturate a low-latency registry. The `networkConcurrency` setting still overrides the default.
