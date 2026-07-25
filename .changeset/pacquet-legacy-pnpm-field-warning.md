---
"pacquet": patch
---

The Rust engine now warns when `package.json` still declares install settings under the `pnpm` field, which pnpm 10 moved to `pnpm-workspace.yaml`. A project that hasn't migrated its `pnpm.overrides` / `pnpm.packageExtensions` / `pnpm.patchedDependencies` previously saw the settings silently ignored, and only met the downstream symptom. Keys the `pnpm` field never owned are left alone.
