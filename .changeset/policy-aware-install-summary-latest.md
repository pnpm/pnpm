---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

The install summary no longer prints `(X is available)` when the registry's `dist-tags.latest` is still held back by the active `minimumReleaseAge` policy. The hint only ever names the actual latest tag, so an immature latest suppresses the hint instead of advertising the version pnpm just refused to install [#11698](https://github.com/pnpm/pnpm/issues/11698).
