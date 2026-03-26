---
"@pnpm/registry.types": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/types": patch
"pnpm": patch
---

Unified registry information types by adding `maintainers` and `contributors` to `PackageInRegistry`. Cleaned up redundant type extensions and casting in the `view` command.
