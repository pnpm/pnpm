---
"pacquet": patch
---

pnpm no longer warns that a `registries` entry routed through the `@` scope matches no configured registry. pnpm applied such an entry and its options such as `serverType` while reporting it as ignored [pnpm/tasks#184](https://github.com/pnpm/tasks/issues/184).
