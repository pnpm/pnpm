---
"pacquet": patch
---

`pnpm update --latest` with package selectors now updates matching transitive dependencies within their declared ranges. Excluded packages and dependencies beyond `--depth` keep their locked versions [#17444](https://github.com/pnpm/pnpm/issues/17444).
