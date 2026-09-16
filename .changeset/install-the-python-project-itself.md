---
"pacquet": minor
---

`pnpm install` now installs a Python project's own package, so the project can be imported and the commands in `[project.scripts]` run right after an install [#14945](https://github.com/pnpm/pnpm/issues/14945). The installed package points at the source tree, so an edit to a module takes effect without another install. pnpm installs the package of a project that declares a `[build-system]`. `tool.uv.package` overrides that either way.
