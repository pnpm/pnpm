---
"pacquet": patch
---

`pnpm install` retries registry metadata fetches that fail with a timeout, a dropped connection, or an interrupted response body before it applies `trustPolicy` or `minimumReleaseAge`. A transient fetch failure is not reported as `TRUST_DOWNGRADE` or `MINIMUM_RELEASE_AGE_VIOLATION` [pnpm/pnpm#12031](https://github.com/pnpm/pnpm/issues/12031).
