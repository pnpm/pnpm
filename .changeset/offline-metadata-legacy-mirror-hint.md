---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

If an offline install fails because the registry metadata cache uses the layout from before pnpm 11.27 and 12.4, the error now names the older mirror on disk and explains that one online install repopulates the cache [#15656](https://github.com/pnpm/pnpm/issues/15656).
