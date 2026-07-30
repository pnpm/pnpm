---
"@pnpm/core-loggers": minor
"@pnpm/hooks.read-package-hook": minor
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/cli.default-reporter": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Warn during install when an entry in `overrides` matches no dependency, and add a `pnpm overrides` command for a stricter audit. Closes pnpm/pnpm#10315.

## `pnpm install` warning

The warning is buffered until the `resolution_done` stage and rendered as a single grouped line by the default reporter (e.g. `[WARN] 2 overrides matched no dependency: foo, parent>child`). Set `warnUnusedOverrides: false` in `pnpm-workspace.yaml` to disable the warning — useful for orgs that share a common set of overrides across repos where not every override applies everywhere.

Detection runs on every TypeScript-pnpm install by scanning the resolved lockfile, including ones that short-circuit against an up-to-date lockfile (`Already up to date`). The scan matches each override selector against the resolved lockfile's importer dependencies, package snapshots, and workspace manifest peer dependencies. A known limitation: version-scoped overrides whose target name is present in the lockfile but at a version outside the override's source range are not flagged, because the resolved lockfile does not preserve the pre-override declared range. The `pnpm overrides` command (below) closes that gap for overrides that match no declared edge at all.

## `pnpm overrides` command

New top-level command. `pnpm overrides` (or `pnpm overrides check`) runs a full lockfile-only re-resolution with the read-package hook's `onApplied` collector wired in, then reports every override selector that **matched no declared edge**. Exits with code 1 when any unused override is found (CI-friendly). `--json` outputs `{ "unused": [...] }`.

Unlike the install warning's lockfile scan, the command catches overrides whose target name is present in the lockfile but whose source range no longer intersects any manifest's declared range — the strict "override never fires" case. It does NOT detect overrides that fire but are no-ops (the resolver picks the same version with or without the bump); detecting that requires per-override re-resolution.

## Parity gaps (pacquet/Rust)

The pacquet port currently gates the install warning on a full lockfile reanalysis — partial-resolution and short-circuit installs do not warn. The `pnpm overrides` command is TypeScript-only; pacquet has no equivalent. Both gaps are tracked as follow-ups.

## Internal/public API additions

- `@pnpm/core-loggers` — new `pnpm:unused-override` channel (`unusedOverrideLogger`, `UnusedOverrideLog`, `UnusedOverrideMessage`).
- `@pnpm/hooks.read-package-hook` — `createVersionsOverrider` accepts an optional `onApplied` callback that fires per matched override; `createReadPackageHook` threads it as `onOverrideApplied`. `createDependencyOverrider` (the resolver-created peer-edge path) gets an equivalent `onApplied` callback — previously it had none, so peer-edge overrides would have been missing from any collector.
- `@pnpm/installing.commands` — new `overrides` command export (`pnpm overrides` / `pnpm overrides check`, with `--json`).
- `@pnpm/installing.deps-installer` — `InstallDepsOptions` (and `ProcessedInstallOptions`) accepts an optional `onAppliedOverride: (selector: string) => void` that fires per matched override during resolution. `pnpm install` does not set it — it uses the lockfile scan instead, since partial-resolution installs bypass the hook for cached subtrees.
- `@pnpm/cli.default-reporter` — new `reportUnusedOverrides` reporter wired into the client reporter pipeline.
