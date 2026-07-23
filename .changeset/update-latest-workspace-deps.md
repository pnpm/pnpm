---
"pacquet": patch
---

Fixed `pnpm update --latest` failing with `ERR_PNPM_PACKAGE_MANAGER_UPDATE_RESOLVE_LATEST` when a dependency uses the `workspace:` (or `link:` / `file:`) protocol. Such a dependency links a local package that may not be published, so there is no registry "latest" to resolve — it is now skipped and preserved verbatim, matching the TypeScript CLI. Previously only `workspace:<path>` specifiers were skipped, so `workspace:*` / `workspace:^1.0.0` deps pointing at unpublished packages made `--latest` try to fetch them from the registry and 404.
