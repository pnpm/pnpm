---
"@pnpm/core-loggers": minor
"@pnpm/hooks.read-package-hook": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/cli.default-reporter": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Warn during install when an entry in `overrides` matches no dependency. Closes pnpm/pnpm#10315.

The warning is buffered until the `resolution_done` stage and rendered as a single grouped line by the default reporter (e.g. `[WARN] 2 overrides matched no dependency: foo, parent>child`). Set `warnUnusedOverrides: false` in `pnpm-workspace.yaml` to disable the warning — useful for orgs that share a common set of overrides across repos where not every override applies everywhere.

Detection runs on every TypeScript-pnpm install by scanning the resolved lockfile, including ones that short-circuit against an up-to-date lockfile (`Already up to date`). The scan matches each override selector against the resolved lockfile's importer dependencies, package snapshots, and workspace manifest peer dependencies. A known limitation: version-scoped overrides whose target name is present in the lockfile but at a version outside the override's source range are not flagged, because the resolved lockfile does not preserve the pre-override declared range.

The pacquet port currently gates detection on a full lockfile reanalysis — partial-resolution and short-circuit installs do not warn. Parity with the TypeScript path is tracked as a follow-up.

Internal/public API additions that support the feature:

- `@pnpm/core-loggers` — new `pnpm:unused-override` channel (`unusedOverrideLogger`, `UnusedOverrideLog`, `UnusedOverrideMessage`).
- `@pnpm/hooks.read-package-hook` — `createVersionsOverrider` accepts an optional `onApplied` callback that fires per matched override; `createReadPackageHook` threads it as `onOverrideApplied`. The exported `VersionOverrideWithoutRawSelector` alias is kept for backward compatibility.
- `@pnpm/installing.deps-installer` — `ProcessedInstallOptions` exposes an `appliedOverrides: Set<string>` so callers can read which override selectors matched after resolution.
- `@pnpm/cli.default-reporter` — new `reportUnusedOverrides` reporter wired into the client reporter pipeline.
