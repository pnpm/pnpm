---
"@pnpm/config.reader": patch
"pnpm": patch
---

Global installs respect the configured build policy (e.g., `dangerouslyAllowAllBuilds`) when the global virtual store is enabled [#9249](https://github.com/pnpm/pnpm/issues/9249).


