---
"pnpm": patch
"pacquet": patch
---

`pnpm fetch` now also installs the pnpm version that `pnpm-lock.yaml` pins, when it differs from the running pnpm. A later `pnpm install --offline` that switches to the pinned version no longer fails because that version is missing from the store [#11808](https://github.com/pnpm/pnpm/issues/11808).
