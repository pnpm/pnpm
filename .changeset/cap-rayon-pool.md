---
"pacquet": patch
---

`pnpm install` now uses at most 16 worker threads for linking by default. On machines with more than 8 cores this lowers CPU use, and on many-core Windows machines warm installs are up to 10% faster.
