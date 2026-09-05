---
"pacquet": minor
---

`pnpm install` and `pnpm add pypi:<package>` now manage Python wheel dependencies when `python.enabled` is set in `pnpm-workspace.yaml`. Python dependencies use `pylock.toml` and a managed `.venv`. `pnpm run` and `pnpm exec` include that environment's executables [#14566](https://github.com/pnpm/pnpm/issues/14566).
