---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm update` rewriting exact version pins that use the `=` operator (for example `=3.5.1`) to a caret range (`^3.5.1`). Exact pins are now preserved and written back as the bare version. See pnpm/pnpm#12745.
