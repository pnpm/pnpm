---
"pacquet": patch
---

On arm64 musl Linux, such as Alpine on ARM, switching to a pinned pnpm older than 12 now runs the JavaScript `pnpm` package. The standalone executable of those versions crashed at startup on that platform [#10443](https://github.com/pnpm/pnpm/issues/10443).
