---
"pacquet": patch
---

`pnpm install` now honors uv workspace members when discovering Python projects. When no uv workspace declares a project, pnpm skips projects under conventional example, demo, documentation, template, `test`, `tests`, and test fixture directories [pnpm/pnpm#15058](https://github.com/pnpm/pnpm/issues/15058).
