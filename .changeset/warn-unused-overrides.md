---
"@pnpm/core-loggers": minor
"@pnpm/hooks.read-package-hook": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/cli.default-reporter": minor
"pnpm": minor
---

Warn during install when an entry in `overrides` matches no dependency. Closes pnpm/pnpm#10315.

The warning is buffered until the `resolution_done` stage and rendered as a single grouped line by the default reporter (e.g. `[WARN] 2 overrides matched no dependency: foo, parent>child`). The same behavior is in pacquet.

Internal/public API additions that support the feature:

- `@pnpm/core-loggers` — new `pnpm:unusedOverride` channel (`unusedOverrideLogger`, `UnusedOverrideLog`, `UnusedOverrideMessage`).
- `@pnpm/hooks.read-package-hook` — `createVersionsOverrider` accepts an optional `onApplied` callback that fires per matched override; `createReadPackageHook` threads it as `onOverrideApplied`. The exported `VersionOverrideWithoutRawSelector` alias is kept for backward compatibility.
- `@pnpm/installing.deps-installer` — `ProcessedInstallOptions` exposes an `appliedOverrides: Set<string>` so callers can read which override selectors matched after resolution.
- `@pnpm/cli.default-reporter` — new `reportUnusedOverrides` reporter wired into the client reporter pipeline.
