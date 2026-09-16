---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm sbom` filtered to a single workspace project now inherits the `author`, `description`, `license`, `repository`, and `bugs` fields from the workspace root manifest. A project that declares one of these fields keeps its own value, even when that value is blank [#14882](https://github.com/pnpm/pnpm/issues/14882).
