---
"pacquet": minor
---

The members of a uv workspace can now share one Python environment. Set `shared-environment = true` under `[tool.pnpm.python]` in the `pyproject.toml` that declares `[tool.uv.workspace]`. `pnpm install` then resolves every member as one graph into one `pylock.toml` and one `.venv` at the workspace root. Two members that require versions of a distribution no release satisfies at once are refused with an error naming both. Each project still gets an environment of its own by default [#15015](https://github.com/pnpm/pnpm/issues/15015).
