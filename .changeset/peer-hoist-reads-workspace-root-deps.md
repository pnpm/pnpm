---
"pacquet": patch
---

Auto-installed peer dependencies are resolved correctly again in a workspace, so a peer that the workspace root already provides is no longer installed a second time.

Two fixes to peer hoisting:

- With `resolvePeersFromWorkspaceRoot` enabled (the default), a missing peer is matched against the **workspace root** project's dependencies. It was matched against the dependencies of whichever project was being resolved, so a project that didn't declare the peer itself resolved a second copy from the registry instead of reusing the root's. In [vercel/next.js](https://github.com/vercel/next.js), whose `overrides` pin `react` to a single canary build, this pulled in a second `react` and paired it with `react-dom` from the canary — a combination the pin exists to prevent.
- A missing **optional** peer is no longer satisfied by a prerelease version that its declared range doesn't accept. `ts-jest`, which declares `@jest/transform` and `jest-util` as optional peers with `^29.0.0 || ^30.0.0`, was bound to `30.0.0-alpha.6` when a `jest` 30 prerelease was elsewhere in the graph, while `jest` itself stayed on 29.
