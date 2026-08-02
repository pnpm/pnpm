---
"pacquet": patch
---

Tarball extraction concurrency now respects the container's CPU quota instead of the host's core count. In a CPU-limited container — a CI runner, a Docker `--cpus` limit — pnpm sized the concurrent-decompression cap and the CAS-write thread pool for every core on the machine rather than the few it was allowed to use, adding scheduler contention and holding more decompression buffers in memory than the container had cores to work through.
