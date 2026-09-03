---
"pacquet": patch
---

`pnpm run` and `pnpm exec` no longer re-run install after a Rust `pnpm install` on filesystems with sub-millisecond mtimes [pnpm/pnpm#14486](https://github.com/pnpm/pnpm/issues/14486).
