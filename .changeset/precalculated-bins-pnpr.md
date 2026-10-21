---
"pacquet": patch
"@pnpm/pnpr": patch
---

Sped up bin linking during `pnpm install` by precalculating package bin targets and shebang runtimes during tarball extraction and in `pnpr` metadata.
