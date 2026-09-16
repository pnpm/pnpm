---
"pacquet": minor
---

Python projects can now select extras and dependency groups through `[tool.pnpm.python]` in `pyproject.toml`. Workspace `python.extras` and `python.groups` defaults now skip names a project does not define. Related to [pnpm/pnpm#14945](https://github.com/pnpm/pnpm/issues/14945).
