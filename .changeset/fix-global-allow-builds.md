---
"@pnpm/config.reader": patch
"pnpm": patch
---

**fix**: global installs respect global config build policy (e.g., `dangerouslyAllowAllBuilds` from config.yaml) when GVS is enabled [#9249](https://github.com/pnpm/pnpm/issues/9249).

The global virtual-store (GVS) default `allowBuilds = {}` was applied before workspace manifest settings were read and before global config values (stripped by `extractAndRemoveDependencyBuildOptions`) were re-applied via `globalDepsBuildConfig`. This caused `hasDependencyBuildOptions` to return `true` (because `{}` is not null), blocking restoration of global config values like `dangerouslyAllowAllBuilds`. As a result, global installs skipped all build scripts even when the config explicitly allowed them.

This fix moves the GVS default to **after** workspace manifest reading and `globalDepsBuildConfig` re-application, so that:
1. Workspace manifest `allowBuilds` takes precedence (if present)
2. Global config `dangerously-allow-all-builds` is properly restored (if set and no workspace policy exists)
3. Empty `{}` is only applied as a last resort when no policy is configured anywhere
