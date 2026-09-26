---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

`pnpm sbom` filtered to a single workspace project no longer replaces the project's own `license` or `bugs` field with the workspace root's value when the project's value is blank. The same applies to an `author`, `description`, `license`, `repository`, or `bugs` field set to `null` [#14882](https://github.com/pnpm/pnpm/issues/14882).
