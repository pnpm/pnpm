---
"pacquet": minor
---

`pnpm install` now supports Python dependencies from Git repositories and direct wheel URLs, including `[tool.uv.sources]` declarations. Git dependencies require `allowBuilds` approval.

Related to [pnpm/pnpm#14945](https://github.com/pnpm/pnpm/issues/14945).
