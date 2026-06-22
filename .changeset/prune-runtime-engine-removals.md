---
"@pnpm/workspace.project-manifest-reader": patch
"pnpm": patch
---

Removing a runtime dependency now removes the matching `devEngines.runtime` or `engines.runtime` entry that was materialized from it.
