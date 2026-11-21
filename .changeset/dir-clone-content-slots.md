---
"pacquet": patch
---

Sped up `pnpm install` on macOS for packages installed with several sets of peer dependencies. pnpm now clones every peer variant of a package from one cached copy of its files.
