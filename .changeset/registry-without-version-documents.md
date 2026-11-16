---
"pacquet": patch
---

When the `pnpm` package downloads its native binary on first run, it now works with registries that serve no per-version package documents, such as Azure Artifacts. It reads the version from the package document instead, and when the registry has removed npm's signatures, it fetches them from registry.npmjs.org. The binary is still downloaded from the configured registry and checked against npm's signature.
