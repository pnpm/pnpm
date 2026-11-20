---
"@pnpm/deps.compliance.sbom": patch
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm sbom` no longer emits components for optional platform-specific dependencies that cannot be installed on the current platform (for example, the native `@rolldown/binding-*` variants for other operating systems). Such packages are present in the lockfile but are never downloaded, so their license (and other metadata) could not be resolved and they appeared in the SBOM without one. `pnpm sbom --lockfile-only` still describes the whole lockfile graph, which is platform-independent by design.
