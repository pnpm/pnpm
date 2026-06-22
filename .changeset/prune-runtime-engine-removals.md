---
"@pnpm/workspace.project-manifest-reader": patch
"@pnpm/pkg-manifest.utils": patch
"@pnpm/engine.runtime.node-resolver": patch
"@pnpm/engine.runtime.bun-resolver": patch
"@pnpm/engine.runtime.deno-resolver": patch
"pnpm": patch
---

Removing a runtime dependency now removes the matching `devEngines.runtime` or `engines.runtime` entry that was materialized from it. Blank runtime selectors are normalized to `latest`.
