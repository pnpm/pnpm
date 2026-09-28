---
"pacquet": patch
---

`pnpm install --frozen-lockfile`, the default in CI, now uses less CPU on machines with more than 8 cores. Warm installs on many-core Windows machines got up to 10% faster. Frozen installs now link with at most 16 worker threads.
