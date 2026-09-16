---
"pacquet": minor
---

`pnpm install` now installs a Python project's own package into its environment, so the project can be imported and the commands in `[project.scripts]` can be run right after an install [#14945](https://github.com/pnpm/pnpm/issues/14945). The package points at the source tree, the way `pip install -e .` and `uv sync` install it, so an edit to a module takes effect without another install. A project is installed when it declares a `[build-system]`, and `tool.uv.package` overrides that either way.
