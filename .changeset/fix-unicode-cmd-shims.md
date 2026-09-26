---
"@pnpm/bins.cmd-shim": patch
"pnpm": patch
"pacquet": patch
---

Fixed Windows command shims failing to run tools whose paths contain non-ASCII characters [#6999](https://github.com/pnpm/pnpm/issues/6999).
