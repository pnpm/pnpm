---
"pacquet": minor
---

`pnpm install`, `pnpm add`, and `pnpm deploy` accept two new flags, each doing one half of what `--force` does. `--reinstall` relinks every package, including ones an earlier install already linked. `--ignore-platform-checks` installs packages whose `os`, `cpu`, `libc`, or `engines` don't match the host [#15094](https://github.com/pnpm/pnpm/issues/15094).
