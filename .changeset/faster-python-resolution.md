---
"pacquet": patch
---

`pnpm install` resolves Python dependencies faster. With `--lockfile-only`, a project with 10 dependencies now resolves in about 4 seconds. It took about 40 seconds before. pnpm now reads index pages and metadata files in parallel. When the index publishes a metadata file beside a wheel (PEP 658), as PyPI does, pnpm downloads only the wheels of the releases it selects.
