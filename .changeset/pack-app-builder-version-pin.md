---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fix `pnpm pack-app` producing SEA executables that crash at startup with a native assertion (`SeaDeserializer::Read() ... format_value <= kModule`).

The Node.js SEA blob format changed in v25.7.0 (ESM entry-point support added a `ModuleFormat` header byte). When the builder Node.js version differed from the embedded runtime version, the blob written by one side could not be deserialized by the other. `pack-app` now always uses a builder Node.js of the exact same version as the embedded runtime, downloading a host-arch copy if the running Node.js doesn't already match.

This regression shipped in `@pnpm/exe@11.0.0-rc.4`, where the CI host Node.js (v25.6.1) built blobs embedded in a v25.9.0 runtime.
