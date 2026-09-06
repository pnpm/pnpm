---
"pacquet": patch
---

pnpm now records the pinned pnpm version in the lockfile the project actually uses when `lockfileDir` is set. It wrote the pin into a second `pnpm-lock.yaml` at the workspace root, and the real lockfile never carried it [#14575](https://github.com/pnpm/pnpm/issues/14575).
