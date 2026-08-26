---
"pacquet": patch
---

Workspace packages declared with a parent-relative pattern in `pnpm-workspace.yaml` (`../shared`, `../../docs/*`) are discovered again. They were dropped from the project list, so `pnpm list -r` and `--filter` did not see them and a frozen install of a lockfile that already held their importer entries failed with `ERR_PNPM_PACKAGE_MANAGER_UNSAFE_IMPORTER_PATH`.
