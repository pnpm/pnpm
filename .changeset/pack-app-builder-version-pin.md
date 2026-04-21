---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fix the `@pnpm/exe` SEA executable crashing at startup on Node.js v25.7+. Two separate regressions in `@pnpm/exe@11.0.0-rc.4` are addressed:

1. `pnpm pack-app` now pins the Node.js used to write the SEA blob to the exact embedded runtime version. The SEA blob format changed in Node.js v25.7 (ESM entry-point support added a `ModuleFormat` header byte), so a blob produced by a pre-25.7 builder cannot be deserialized by a 25.7+ runtime and vice versa. In rc.4 the CI host Node.js (v25.6.1) built blobs embedded in a v25.9.0 runtime, tripping `SeaDeserializer::Read() ... format_value <= kModule` on every invocation. `pack-app` now downloads a host-arch builder Node.js of the target version when the running Node.js doesn't already match.

2. The pnpm CJS SEA entry shim now loads `dist/pnpm.mjs` through `Module.createRequire(process.execPath)` instead of `await import(pathToFileURL(...).href)`. In Node.js v25.7+, the ambient `require` and `import()` inside a CJS SEA entry are replaced with embedder hooks that only resolve built-in module names, causing external `file://` loads to fail with `ERR_UNKNOWN_BUILTIN_MODULE`. An explicit `createRequire()` bypasses those hooks.
