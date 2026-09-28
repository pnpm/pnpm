---
"pacquet": patch
---

`pnpm install --frozen-lockfile`, the default in CI, now links with at most 16 worker threads. On machines with more than 8 cores this lowers CPU use, and on many-core Windows machines warm installs are up to 10% faster.
