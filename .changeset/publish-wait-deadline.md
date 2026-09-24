---
"pacquet": patch
---

`pnpm publish` stops polling for package availability as soon as `--publish-wait-timeout` expires [pnpm/pnpm#15387](https://github.com/pnpm/pnpm/issues/15387). A timed-out publish previously sent an extra registry metadata request after the deadline.
