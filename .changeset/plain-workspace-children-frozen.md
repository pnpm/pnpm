---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now accepts a plain-range dependency of a local directory dependency that `linkWorkspacePackages: deep` linked to a workspace project, and rejects the lockfile when its `injected` setting no longer matches the recorded reference.
