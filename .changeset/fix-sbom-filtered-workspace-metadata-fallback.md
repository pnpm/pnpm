---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm sbom` when filtered to a single workspace project now inherits missing root metadata (`author`, `description`, `license`, `repository`, and `bugs`) from the workspace root manifest [#14882](https://github.com/pnpm/pnpm/issues/14882).
