---
"pacquet": patch
---

A peer dependency that the workspace root already provides is no longer installed a second time. With `resolvePeersFromWorkspaceRoot` enabled (the default), a missing peer is matched against the **workspace root** project's dependencies; it was matched against the dependencies of whichever project was being resolved, so a project that didn't declare the peer itself resolved its own copy from the registry. In [vercel/next.js](https://github.com/vercel/next.js), whose `overrides` pin `react` to a single canary build, this pulled in a second `react` and paired it with `react-dom` from the canary — a combination the pin exists to prevent.
