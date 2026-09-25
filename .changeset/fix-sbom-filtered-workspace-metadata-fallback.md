---
"pacquet": patch
---

`pnpm sbom` filtered to a single workspace project now takes the `author`, `description`, `license`, `repository`, and `bugs` fields from the workspace root `package.json` when the project does not declare them. A field the project declares is never taken from the root, even when it is blank or `null` [#14882](https://github.com/pnpm/pnpm/issues/14882).
