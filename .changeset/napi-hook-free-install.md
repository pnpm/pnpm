---
"pacquet": minor
---

Added `projects[].dependencyManifest` to the `@pnpm/napi` install options: the manifest a workspace project exposes when it is resolved as a dependency of another importer (an injected instance). Hosts that pre-transform their importer manifests no longer need a `readPackage` hook to substitute the raw manifest, and per-manifest deletions are expressed through the existing `overrides` removal syntax (`"pkg": "-"`), so resolution can run without any JS round trips.
