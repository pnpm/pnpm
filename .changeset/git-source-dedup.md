---
"pacquet": patch
---

Installing several packages from the same Git repository and commit now downloads the source once per install. Each package still runs its prepare scripts in its own copy of the checkout [pnpm/pnpm#14725](https://github.com/pnpm/pnpm/issues/14725).
