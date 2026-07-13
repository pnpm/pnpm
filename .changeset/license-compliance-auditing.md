---
"@pnpm/deps.compliance.license-checker": minor
"@pnpm/deps.compliance.commands": minor
"@pnpm/types": minor
"@pnpm/workspace.workspace-manifest-reader": minor
"@pnpm/config.reader": patch
"@pnpm/installing.commands": minor
"pnpm": minor
---

Added built-in license compliance auditing via `licenses` in `pnpm-workspace.yaml`. New subcommands: `pnpm licenses check`, `pnpm licenses allow`, `pnpm licenses disallow` [#10570](https://github.com/pnpm/pnpm/issues/10570).

When a `licenses` policy is configured, `pnpm add` and `pnpm update` (including `pnpm add --config`) now run the license check after installing and fail with `ERR_PNPM_LICENSE_VIOLATION` if a dependency uses a disallowed license. Plain `pnpm install` is intentionally not gated.

In loose mode, a license that is not in the configured `allowed` list is now reported as a warning instead of being silently allowed — the add/update still succeeds, but the warning is printed (and shows up in `pnpm licenses check`).
