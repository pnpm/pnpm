---
"@pnpm/pnpr": patch
---

A registry pattern that is not a valid package name now gets an error that names the patterns its ecosystem accepts. The error used to suggest `@scope/*` for every ecosystem, but only npm registries accept it.
