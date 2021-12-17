---
"@pnpm/resolve-dependencies": patch
---

Don't warn about unmet peer dependency when the peer is resolved from a prerelease version.

For instance, if a project has `react@*` as a peer dependency, then react `16.0.0-rc.0` should not cause a warning.
