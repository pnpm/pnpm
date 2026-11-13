---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Unchanged installs now preserve peer contexts when peer-dependent packages are deduplicated. Removing a workspace dependency preserves nested peer providers still required by the remaining packages.

Unchanged installs retain valid locked peer providers even when a direct dependency provides another compatible version. Explicit `pnpm add` and `pnpm update` requests take precedence over those retained bindings.
