---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Don't promote a `runtime:` dependency (such as the Node.js version from `devEngines.runtime` or `pnpm runtime set`) into a catalog when `catalogMode` is `strict` or `prefer`. A `runtime:` dependency round-trips to `devEngines.runtime`, which only recognizes the `runtime:` protocol; cataloging it rewrote the manifest entry to `catalog:`, which broke that round-trip, stranded it in `devDependencies`, and left `devEngines.runtime` untouched.
