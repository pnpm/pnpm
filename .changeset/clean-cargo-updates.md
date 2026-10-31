---
"pacquet": patch
---

`pnpm update` keeps an exact `=` Cargo requirement exact, and saved requirements omit build metadata [pnpm/tasks#243](https://github.com/pnpm/tasks/issues/243).

Negated `crate:` and `pypi:` selectors, such as `!crate:sha2`, now exclude that dependency and update the rest. `pnpm update` also resolves a Cargo workspace once, not once per member.
