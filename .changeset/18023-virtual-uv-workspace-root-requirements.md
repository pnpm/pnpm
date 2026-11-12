---
"pacquet": patch
---

A `requirements.txt` beside a `pyproject.toml` that declares `[tool.uv.workspace]` without a `[project]` table no longer makes that directory a Python project. `pnpm install` stopped there with an error when the directory had a `.venv` that pnpm did not create [#18023](https://github.com/pnpm/pnpm/issues/18023).
