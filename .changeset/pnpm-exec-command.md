---
"@pnpm/config.reader": minor
"@pnpm/types": minor
"pnpm": minor
---

Added a `pnpmExecCommand` setting (in `pnpm-workspace.yaml`). It is a command, given as an argv array, that prints the absolute path of the pnpm binary the project must run under. pnpm runs it once per user invocation and re-executes into the returned binary when it differs from the running one. This lets an external version manager that already has pnpm on disk pin the exact binary — without the `packageManager` field of `package.json` (no manifest churn, no registry download) and without breaking corepack.

When `pnpmExecCommand` is set, it owns binary selection: download-based version switching from `packageManager`/`devEngines.packageManager` is skipped, and those fields are instead validated against the binary the command resolved. A version mismatch reports that the binary was selected by `pnpmExecCommand`. Projects using this setting are encouraged to declare `devEngines.packageManager` with a version range for documentation and validation.
