---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

pnpm now fails with `ERR_PNPM_REGISTRY_NAME_MISMATCH` if a registry returns metadata for a package other than the one requested. Before, pnpm installed the package under the name in the metadata, so `allowBuilds` rules for the requested package did not apply to it.
