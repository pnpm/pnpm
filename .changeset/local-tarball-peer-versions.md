---
"pacquet": patch
---

`pnpm install` and `pnpm peers check` now use local tarball packages' actual versions when checking peer dependencies. Compatible packages no longer fail with `strictPeerDependencies` enabled.
