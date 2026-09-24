---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm publish` and `pnpm pack` now report an error when a bin script has a shebang line ending with CRLF [pnpm/pnpm#7311](https://github.com/pnpm/pnpm/issues/7311).
