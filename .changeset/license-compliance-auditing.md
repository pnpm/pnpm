---
"@pnpm/deps.compliance.license-checker": minor
"@pnpm/deps.compliance.commands": minor
"@pnpm/types": minor
"@pnpm/workspace.workspace-manifest-reader": minor
"@pnpm/config.reader": patch
"@pnpm/installing.commands": minor
"@pnpm/global.commands": minor
"pnpm": minor
---

Added built-in license compliance auditing via `licenses` in `pnpm-workspace.yaml`. New subcommands: `pnpm licenses check`, `pnpm licenses allow`, `pnpm licenses disallow` [#10570](https://github.com/pnpm/pnpm/issues/10570).

When a `licenses` policy is configured, `pnpm add` and `pnpm update` (including `pnpm add --config`) now run the license check after installing and fail with `ERR_PNPM_LICENSE_VIOLATION` if a dependency uses a disallowed license. Plain `pnpm install` is intentionally not gated.

In loose mode, a license that is not in the configured `allowed` list is now reported as a warning instead of being silently allowed — the add/update still succeeds, but the warning is printed (and shows up in `pnpm licenses check`).

Global installs (`pnpm add -g`, `pnpm update -g`) now enforce the same `licenses` policy. Each CLI param becomes its own isolated install group under the global package dir, so the license check now runs once per group right after that group installs; a disallowed license fails with `ERR_PNPM_LICENSE_VIOLATION` and the violating group's install directory is removed, leaving previously-installed global packages untouched. The policy is read from the `pnpm-workspace.yaml` at the global package directory, same as the rest of the global-install config.

Compound (AND/OR) SPDX license expressions (e.g. `"GPL-3.0-only OR GPL-2.0-only"`) are rejected in the `allowed`/`disallowed` lists both by the `pnpm licenses allow`/`disallow` CLI and, now, whenever a license check runs — closing a gap where a compound hand-edited directly into `licenses.allowed`/`licenses.disallowed` in `pnpm-workspace.yaml` could bypass the CLI's validation. On the disallow side this previously fail-opened silently (the compound never matched any single license and nothing was blocked); it now fails fast with `ERR_PNPM_LICENSES_COMPOUND_EXPRESSION`. List each license identifier separately instead, e.g. `["GPL-3.0-only", "GPL-2.0-only"]`.
