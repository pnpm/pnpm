---
"pacquet": patch
---

Validate the project's pinned package manager and runtimes before running a command, matching the pnpm CLI:

- A `packageManager` / `devEngines.packageManager` pin that the running pnpm does not satisfy now fails with `ERR_PNPM_BAD_PM_VERSION` (or `ERR_PNPM_OTHER_PM_EXPECTED` when the project is pinned to another package manager), instead of being silently ignored. The check also runs under corepack, where pnpm cannot switch versions itself, and says so.
- `devEngines.runtime` / `engines.runtime` entries with `onFail: "error"` or `onFail: "warn"` are validated against the Node.js, Deno, or Bun installed on the system, failing with `ERR_PNPM_BAD_RUNTIME_VERSION`.
- `pmOnFail` and `runtimeOnFail` are honored as bypasses and can now be passed as `--pm-on-fail=<value>` / `--runtime-on-fail=<value>`, the form the error hints suggest.

Global commands (`--global`) and commands that do not belong to the project (`store`, `dlx`, `self-update`, …) skip these checks, as does a project pin that only asked pnpm to switch versions when `manage-package-manager-versions` is turned off.
