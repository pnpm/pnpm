---
"pacquet": patch
---

Bounded the number of requests in flight to the `.pnpmfile.cjs` worker process. An install that runs the `readPackage` hook for thousands of packages at once no longer keeps every request's payload in memory, and no longer risks failing with `ERR_PNPM_PNPMFILE_FAIL` on a hook timeout spent queueing rather than running the hook.
