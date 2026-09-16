---
"pacquet": minor
---

`pnpm install` now supports Python git requirements and direct wheel URLs, including sources declared in `[tool.uv.sources]`. Git sources are locked to a full commit and wheel URLs are locked with a SHA-256 hash. Frozen and offline installs replay these sources.

Approve a git dependency and its build requirements under `allowBuilds` using Package URLs such as `pkg:pypi/talon-core: true`.

Related to [pnpm/pnpm#14945](https://github.com/pnpm/pnpm/issues/14945).
