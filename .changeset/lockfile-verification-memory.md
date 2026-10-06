---
"pacquet": patch
---

Verifying the lockfile against `trustPolicy` and `minimumReleaseAge` uses less memory. pnpm no longer keeps every published version's manifest of each checked package in memory until the install ends [#16656](https://github.com/pnpm/pnpm/issues/16656).
