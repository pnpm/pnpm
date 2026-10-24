---
"pacquet": minor
---

Added a `--no-modify-path` option to `pnpm setup`. It installs pnpm without adding it to `PATH`, leaving the shell config file, the Windows registry, and the GitHub Actions environment files unchanged. `pnpm setup` then prints the lines to add yourself [get.pnpm.io#37](https://github.com/pnpm/get.pnpm.io/issues/37).
