---
"pacquet": minor
---

`pnpm outdated` and `pnpm update` now inspect and update package manager and runtime versions in `devEngines.packageManager`, `packageManager`, `devEngines.runtime`, `engines.runtime`, and `runtime:` dependencies. With `cargo.enabled` or `python.enabled`, `pnpm outdated` and `pnpm update` also inspect and update Cargo dependencies in `Cargo.toml` and `Cargo.lock` and Python dependencies in `pyproject.toml` and `pylock.toml`.
