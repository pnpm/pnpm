---
"pnpm": patch
"pacquet": patch
---

`pnpm setup` now removes leftover v10-layout shims at the top of `PNPM_HOME`, so `pnpm self-update` no longer warns about a v10 installation layout after PATH has been migrated to the v11 `PNPM_HOME/bin` layout. Applies to both the TypeScript CLI and pacquet.

In the TypeScript CLI, `self-update` also no longer treats a dangling legacy shim (one whose install target was garbage-collected) as a real v10 layout, so the warning can no longer fire on dead shim files.

Closes pnpm/pnpm#12496.
