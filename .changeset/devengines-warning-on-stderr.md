---
"pnpm": patch
"pacquet": patch
---

A warning about a project's `devEngines` or `packageManager` pin is now printed to stderr. A command such as `pnpm cache path` or `pnpm list --json` keeps only its own output on stdout [#16584](https://github.com/pnpm/pnpm/issues/16584).
