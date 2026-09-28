---
"pacquet": patch
---

An `npm:` alias written by `overrides` now stays in place when a change elsewhere makes pnpm re-resolve the aliased dependency. Before, pnpm could look up the alias name at the aliased version, which failed with `ERR_PNPM_NO_MATCHING_VERSION` or locked an unrelated package [#16309](https://github.com/pnpm/pnpm/issues/16309).
