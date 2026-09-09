---
"pacquet": patch
---

Installing multiple packages from the same Git repository and commit now downloads the source once per install. Each package still runs its preparation scripts in a separate directory. [pnpm/pnpm#14725](https://github.com/pnpm/pnpm/issues/14725).
