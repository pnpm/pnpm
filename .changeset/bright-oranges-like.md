---
"@pnpm/tarball-resolver": patch
---

Dependencies specified via a URL that redirects will only be locked to the target if it is immutable, fixing a regression when installing from GitHub releases. ([#9531](https://github.com/pnpm/pnpm/issues/9531))
