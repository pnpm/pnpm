---
"pacquet": minor
---

`pnpm install` and `pnpm add pypi:<package>` now manage Python wheel dependencies when `python.enabled` is set in `pnpm-workspace.yaml`. Python dependencies use `pyproject.toml`, the standard `pylock.toml` lockfile, and a managed `.venv`. Frozen and offline installs are supported. `pnpm run` and `pnpm exec` include that environment's executables.

Mixed-ecosystem installs wait for all enabled ecosystems before publishing Python state. Failed publication restores Python metadata and the previous environment. Python workspace discovery excludes configured stores and caches.

Network and archive retry logs hide credentials and signed query parameters in request URLs.

[pnpm/pnpm#14566](https://github.com/pnpm/pnpm/issues/14566)
