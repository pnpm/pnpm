---
"pacquet": minor
---

`pnpm install` now supports Python dependencies from Git repositories. Direct wheel URLs are also supported. Sources can be declared in `[tool.uv.sources]`. Git dependencies require `allowBuilds` approval.

Related to [pnpm/pnpm#14945](https://github.com/pnpm/pnpm/issues/14945).
