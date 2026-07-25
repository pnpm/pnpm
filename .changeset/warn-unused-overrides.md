---
"@pnpm/core-loggers": minor
"@pnpm/hooks.read-package-hook": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/cli.default-reporter": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Warn during install when an entry in `overrides` matches no dependency. Closes pnpm/pnpm#10315.

The warning is buffered until the `resolution_done` stage and rendered as a single grouped line by the default reporter (e.g. `[WARN] 2 overrides matched no dependency: foo, parent>child`). Set `warnUnusedOverrides: false` in `pnpm-workspace.yaml` to disable the warning — useful for orgs that share a common set of overrides across repos where not every override applies everywhere. The same behavior is in pacquet.

Detection only runs during a full lockfile reanalysis: the applied-override set is collected by the read-package hook as manifests stream through resolution, so any install that short-circuits against a cached lockfile (`--frozen-lockfile`, `--prefer-frozen-lockfile`, or a partial resolution that reuses prior subtrees) skips the check. In those modes, unused overrides are not reported.

Internal/public API additions that support the feature:

- `@pnpm/core-loggers` — new `pnpm:unused-override` channel (`unusedOverrideLogger`, `UnusedOverrideLog`, `UnusedOverrideMessage`).
- `@pnpm/hooks.read-package-hook` — `createVersionsOverrider` accepts an optional `onApplied` callback that fires per matched override; `createReadPackageHook` threads it as `onOverrideApplied`. The exported `VersionOverrideWithoutRawSelector` alias is kept for backward compatibility.
- `@pnpm/installing.deps-installer` — `ProcessedInstallOptions` exposes an `appliedOverrides: Set<string>` so callers can read which override selectors matched after resolution.
- `@pnpm/cli.default-reporter` — new `reportUnusedOverrides` reporter wired into the client reporter pipeline.
