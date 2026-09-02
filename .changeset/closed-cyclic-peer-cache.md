---
"pacquet": patch
---

Fixed pnpm generating a broken lockfile when an npm-aliased dependency in a cyclic peer graph reuses a cached resolution [#14449](https://github.com/pnpm/pnpm/issues/14449). Frozen installs now reference the cached occurrence's emitted snapshot.
