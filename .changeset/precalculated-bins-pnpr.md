---
"pacquet": patch
"@pnpm/pnpr": patch
---

Sped up bin linking during `pnpm install`. Package binary targets and shebang runtimes are now precalculated during tarball extraction and included in `pnpr` registry metadata.
