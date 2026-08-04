---
"pacquet": patch
---

A missing optional peer dependency is no longer satisfied by a prerelease version that its declared range doesn't accept. `ts-jest`, which declares `@jest/transform` and `jest-util` as optional peers with `^29.0.0 || ^30.0.0`, was bound to `30.0.0-alpha.6` when a `jest` 30 prerelease was elsewhere in the graph, while `jest` itself stayed on 29.
