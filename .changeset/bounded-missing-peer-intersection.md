---
"pacquet": patch
---

Fixed `pnpm install`, `pnpm add`, `pnpm remove`, and `pnpm peers check` running out of memory when many packages share a missing peer dependency whose ranges have overlapping alternatives, such as `>=1.0.0 || ^1.0.0`. This mostly affected projects with `autoInstallPeers: false` [#15362](https://github.com/pnpm/pnpm/issues/15362).
