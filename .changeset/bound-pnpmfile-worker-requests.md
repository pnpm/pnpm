---
"pacquet": patch
---

Bounded the number of requests in flight to the `.pnpmfile.cjs` worker process. An install that runs the `readPackage` hook for thousands of packages at once no longer risks failing with `ERR_PNPM_PNPMFILE_FAIL` on a hook timeout spent waiting in the queue rather than running the hook, and holds fewer copies of the manifests it is hooking while it waits.
