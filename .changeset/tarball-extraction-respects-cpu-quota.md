---
"pacquet": patch
---

Tarball extraction concurrency now respects the container's CPU quota instead of the host's core count. In a CPU-limited container — a CI runner, a Docker `--cpus` limit — pnpm sized the concurrent-decompression cap and the CAS-write thread pool for every core on the machine, not the few it was allowed to use. Each concurrent decompression holds the tarball and its inflate output in memory, so on a small-memory runner installs could be killed by the OOM killer.
