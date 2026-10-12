---
"pacquet": patch
---

`pnpm install` resolves Python dependencies faster. On a project with 10 dependencies, `pnpm install --lockfile-only` took about 4 seconds instead of about 40. pnpm now reads index pages and metadata files in parallel. When the index publishes a metadata file beside a wheel (PEP 658), as PyPI does, pnpm downloads only the wheels of the releases it selects.
