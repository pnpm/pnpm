---
"@pnpm/npm-resolver": patch
"pnpm": patch
---

fix: treat HTTP 400 responses as errors in the npm resolver fetch

The status check used `> 400` instead of `>= 400`, causing 400 Bad Request responses to bypass the error path and fall into JSON parse/retry logic instead.
