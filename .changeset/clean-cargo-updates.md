---
"pacquet": patch
---

`pnpm update` keeps Cargo dependency requirements valid when updating aliases, patched crates, prereleases, exact pins, and build metadata [pnpm/tasks#243](https://github.com/pnpm/tasks/issues/243).

Negated `crate:` and `pypi:` selectors, such as `!crate:sha2`, now exclude that dependency and update the rest. `pnpm update` also resolves a Cargo workspace once, not once per member.
