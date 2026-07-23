---
"pacquet": patch
---

Fixed `pnpm update --latest` failing with `ERR_PNPM_PACKAGE_MANAGER_UPDATE_RESOLVE_LATEST` when a dependency links a local workspace package that may not be published. Such a dependency has no registry "latest" to resolve, so it is now skipped and preserved verbatim, matching the TypeScript CLI. This covers a dependency declared with the `workspace:` (or `link:` / `file:`) protocol — previously only `workspace:<path>` specifiers were skipped, so `workspace:*` / `workspace:^1.0.0` deps pointing at unpublished packages made `--latest` fetch them from the registry and 404 — as well as a plain-semver dependency that `linkWorkspacePackages` links to a local sibling.
