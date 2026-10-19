---
"pacquet": patch
---

`pnpm install` in a Cargo workspace skips crates that an earlier install already unpacked into the store. Before, a repeat install processed every crate in `Cargo.lock` again. `pnpm install --force` unpacks every crate again [pnpm/tasks#69](https://github.com/pnpm/tasks/issues/69).
