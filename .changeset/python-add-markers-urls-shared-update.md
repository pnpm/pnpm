---
"pacquet": patch
---

`pnpm add` for `pypi:` dependencies now preserves existing entries in `pyproject.toml` whose PEP 508 environment markers differ from the added requirement. `pnpm add` also accepts PEP 508 direct URL requirements such as `pypi:pkg @ https://...`. `pnpm update` at a shared-environment workspace root without a root `[project]` table now updates matching member manifests and the shared `pylock.toml`.
