---
"pacquet": patch
---

When pnpm switches to the version a project pins for `pnpm --version`, or for a command line it does not recognize, it now honors `--color`, `--no-color`, `--reporter`, and `--loglevel` wherever they appear on the command line.
