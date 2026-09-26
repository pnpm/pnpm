---
"@pnpm/patching.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm patch-commit` now resolves default patch directory locations when passed a package name or package specifier (such as `pnpm patch-commit <pkg>` or `pnpm patch-commit <pkg>@<version>`).
