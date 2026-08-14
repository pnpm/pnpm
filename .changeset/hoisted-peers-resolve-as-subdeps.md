---
"pacquet": patch
---

Auto-installed peer dependencies are no longer resolved to their lowest satisfying versions under `resolutionMode: lowest-direct` or `time-based`. A hoisted peer is not a dependency the project declares, so it resolves like a transitive dependency — to the highest version satisfying the peer range (under `time-based`, the highest within the publish-date cutoff) [#13871](https://github.com/pnpm/pnpm/pull/13871).
