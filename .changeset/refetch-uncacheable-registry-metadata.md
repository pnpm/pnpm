---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` and `pnpm update` refetch registry metadata when the registry sends `Cache-Control: max-age=0`, `no-cache`, or `no-store`. A newly published version from that registry is visible on the next lookup [#13487](https://github.com/pnpm/pnpm/issues/13487).
