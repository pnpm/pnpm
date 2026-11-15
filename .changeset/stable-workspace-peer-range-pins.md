---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

Repeated installs now keep automatically installed peer versions stable when workspace projects use different ranges of the same dependency. Adding another ranged provider also preserves a valid peer pin.
