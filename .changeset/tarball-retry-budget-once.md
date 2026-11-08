---
"pacquet": patch
---

A package tarball that keeps failing to download now fails the install after `fetchRetries` retries. pnpm used to download it again with a second full set of retries, which doubled the wait. The error now names the actual download failure.
