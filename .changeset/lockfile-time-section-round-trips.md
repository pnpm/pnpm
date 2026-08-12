---
"pacquet": patch
---

The lockfile's `time:` section is no longer dropped when `pnpm-lock.yaml` is rewritten. `resolutionMode: time-based` records each direct dependency's publish date there and now reads it back as the fallback for a package whose registry metadata carries no publish date, so a later resolution derives the same cutoff instead of picking different subdependency versions [#13776](https://github.com/pnpm/pnpm/issues/13776).
