---
"pacquet": patch
---

`pnpm install` now vendors recursive Git submodules for Cargo dependencies at their pinned commits. Cargo builds can use these sources offline. Existing cached Git crates are fetched again on the first online install [pnpm/pnpm#14951](https://github.com/pnpm/pnpm/issues/14951).
