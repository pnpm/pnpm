---
"pacquet": patch
"get-pnpm": patch
---

When the `pnpm` package downloads its native binary on first run, it now works with registries that serve no per-version package documents, such as Azure Artifacts. If such a registry also removed npm's signatures, pnpm fetches them from registry.npmjs.org. The binary still comes from the configured registry and is checked against npm's signature.
