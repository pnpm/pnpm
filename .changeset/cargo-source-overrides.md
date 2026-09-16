---
"pacquet": patch
---

`pnpm install` can now generate Cargo.lock for workspaces with path or Git `[patch]` and `[replace]` overrides. Adding, removing, and updating crates also preserve these overrides [pnpm/pnpm#14950](https://github.com/pnpm/pnpm/issues/14950).
