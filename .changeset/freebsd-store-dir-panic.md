---
"pacquet": patch
---

pnpm no longer crashes at startup on FreeBSD and other Unix-like platforms. The default store directory is `~/.local/share/pnpm/store` on every platform that is not Windows or macOS [#14859](https://github.com/pnpm/pnpm/issues/14859).
