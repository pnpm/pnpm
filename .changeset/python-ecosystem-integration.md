---
"pacquet": minor
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/network.fetch": patch
"pnpm": patch
---

In pnpm v12, `pnpm install` and `pnpm add pypi:<package>` manage Python wheel dependencies when `python.enabled` is set in `pnpm-workspace.yaml`. Python dependencies use `pyproject.toml`, the standard `pylock.toml` lockfile, and a managed `.venv`. Frozen and offline installs are supported. `pnpm run` and `pnpm exec` include that environment's executables.

Mixed-ecosystem installs wait for all enabled ecosystems before publishing Cargo and Python state. Failed publication restores participating Cargo and Python metadata and the previous Python environment. Workspace discovery excludes configured stores and caches. Cargo registry requests respect the configured fetch retry budget.

In pnpm v11 and v12, network and archive retry logs hide credentials and signed query parameters in request URLs.

[pnpm/pnpm#14566](https://github.com/pnpm/pnpm/issues/14566)
