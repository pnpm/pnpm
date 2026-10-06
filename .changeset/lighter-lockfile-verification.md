---
"pacquet": patch
---

Verifying a lockfile against `trustPolicy` and `minimumReleaseAge` uses much less memory. On a cold cache with a 1,200-entry lockfile, peak memory dropped from about 3.6 GB to about 2.1 GB [#16656](https://github.com/pnpm/pnpm/issues/16656).
