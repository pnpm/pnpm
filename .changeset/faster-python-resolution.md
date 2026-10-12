---
"pacquet": patch
---

Python dependencies resolve much faster. `pnpm install` now reads index pages and metadata files in parallel. When the index publishes a metadata file beside a wheel (PEP 658), as PyPI does, pnpm reads the release's requirements from it and downloads only the wheels of the releases it selects. On a project with 10 dependencies, `pnpm install --lockfile-only` dropped from about 40 seconds to about 4.
