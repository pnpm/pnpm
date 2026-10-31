---
"pacquet": patch
---

`pnpm update` keeps Cargo dependency requirements valid [pnpm/tasks#243](https://github.com/pnpm/tasks/issues/243). A renamed dependency stays within its declared version requirement with `--latest`. Crates replaced under `[patch]` are left unchanged. A prerelease is no longer downgraded to an older stable release. An exact `=` requirement stays exact, and saved requirements omit build metadata.

Negated `crate:` and `pypi:` selectors, such as `!crate:sha2`, now exclude that dependency and update the rest. `pnpm update` also resolves a Cargo workspace once, not once per member.
