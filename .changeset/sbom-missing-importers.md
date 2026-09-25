---
"@pnpm/deps.compliance.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm sbom` now fails with `ERR_PNPM_SBOM_MISSING_IMPORTERS` when `pnpm-lock.yaml` has no entry for a selected project, instead of writing an SBOM that under-reports that project's dependencies. Previously this crashed with `Cannot read properties of undefined (reading 'devDependencies')`.
