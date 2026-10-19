---
"pacquet": patch
---

Repeat `pnpm install` runs in a Cargo workspace no longer process crates that an earlier install already unpacked into the store. Only crates missing from the store are downloaded and unpacked, and `pnpm install --force` unpacks every crate again [pnpm/tasks#69](https://github.com/pnpm/tasks/issues/69).
