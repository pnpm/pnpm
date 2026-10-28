---
"pacquet": patch
---

`pnpm update --latest` with package selectors now updates the dependencies of the selected packages within their declared ranges. Excluded packages and their dependencies keep their locked versions [#17444](https://github.com/pnpm/pnpm/issues/17444).
