---
"pacquet": minor
---

Added three `@pnpm/napi` install options that let embedders express their `readPackage` hook logic engine-side and skip the per-manifest JS round trips entirely:

- `projects[].dependencyManifest` — the manifest to use when a workspace project is resolved as a dependency of another importer (an injected instance), so hosts that pre-transform importer manifests no longer need a hook to substitute the raw manifest.
- `ignoredDependencies` — package names removed from every resolved manifest's `dependencies` (unless the range is a `link:`) and `peerDependencies`, for packages the host environment provides itself.
- `neverBuiltDependencies` — now accepted and folded into the allow-builds policy as explicit denials instead of being rejected as unsupported.
