---
"pacquet": patch
---

Python dependencies resolve much faster. `pnpm install` now reads each release's requirements from the metadata file the index publishes beside its wheel (PEP 658), and reads index pages and metadata files in parallel. Wheels are downloaded only for the releases the resolution selects. On a project with 10 dependencies, `pnpm install --lockfile-only` dropped from about 40 seconds to about 4.
