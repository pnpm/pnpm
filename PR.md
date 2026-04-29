## fix: global installs respect `.npmrc` build policy when GVS is enabled

Closes #9249

### The Problem

`pnpm add -g <pkg>` silently skips build scripts for native packages even when `dangerously-allow-all-builds=true` is set in `.npmrc` or when `allowBuilds` entries exist in the global `pnpm-workspace.yaml`. This breaks any globally installed package with native bindings (e.g., `better-sqlite3`, `esbuild`, `gitnexus`) because its postinstall scripts never run.

### Root Cause

The config reader applies a GVS (global virtual-store) default `allowBuilds = {}` **before** two critical things happen:
1. Reading `allowBuilds` from the global `pnpm-workspace.yaml`
2. Re-applying `.npmrc` settings (like `dangerouslyAllowAllBuilds`) that were previously stripped by `extractAndRemoveDependencyBuildOptions`

When `allowBuilds = {}` (empty but not null), `hasDependencyBuildOptions(pnpmConfig)` returns `true`. This blocks `Object.assign(pnpmConfig, globalDepsBuildConfig)` from restoring `.npmrc`-derived values. The result: build policy is "configured" but empty → all build scripts are skipped.

### The Fix

Move the GVS default from **before** workspace manifest reading to **after** `globalDepsBuildConfig` re-application. Same condition, same behavior — just correct ordering.

**Before (buggy):**
```
1. GVS default: allowBuilds = {}   ← blocks step 3
2. Read workspace manifest         ← may set allowBuilds (correctly overrides step 1)
3. Re-apply .npmrc values        ← BLOCKED because step 1 made allowBuilds non-null
```

**After (fixed):**
```
1. Read workspace manifest         ← workspace policy takes precedence
2. Re-apply .npmrc values        ← restored when no workspace policy exists
3. GVS default: allowBuilds = {}  ← only applied as last resort
```

### Changes

- `config/reader/src/index.ts`: Move GVS `allowBuilds = {}` default to after `globalDepsBuildConfig` re-application (lines ~380-386 → ~494-501)
- `config/reader`: Add changeset for patch release

### Impact

| Scenario | Before | After |
|---|---|---|
| GVS on, workspace manifest has `allowBuilds` | Works (already correct in v11) | Same |
| GVS on, no workspace manifest, `.npmrc` has `dangerously-allow-all-builds` | **Broken**: builds silently skipped | **Fixed**: builds run |
| GVS on, no workspace manifest, no `.npmrc` policy | Prompts after install | Prompts after install |
| GVS off | Works | Works |

### Related Issues

- #9249 — This issue (verified)
- #9073 — Same symptom (closed, likely fixed by the same v11 `else if` branch)
- #9478 — Native bindings fail from skipped builds (likely also fixed by this ordering fix)
- #8891 — Feature request for `onlyBuiltDependencies` global support (separate)
