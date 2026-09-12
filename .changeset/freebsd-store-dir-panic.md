---
"pacquet": patch
---

`pnpm` no longer crashes on startup on FreeBSD and other non-Windows, non-macOS platforms. The default store directory now resolves to `~/.local/share/pnpm/store` on every Unix-like host [#14859](https://github.com/pnpm/pnpm/issues/14859).
