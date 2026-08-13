---
"@pnpm/crypto.shasums-file": minor
"@pnpm/engine.runtime.node-resolver": minor
"@pnpm/resolving.default-resolver": patch
"pnpm": minor
"pacquet": minor
---

Resolving a Node.js runtime version (`devEngines.runtime` / `runtime:` specifiers) is now much faster and no longer needs the network once the version's release metadata has been fetched once. The signature-verified per-version `SHASUMS256.txt` bodies are cached under `<cacheDir>/v11/runtime-shasums/`, and an exact stable version such as `runtime:22.23.2` skips the Node.js release-index download entirely. This cuts the first `node` invocation in a project pinning an already-downloaded runtime from ~650ms to ~100ms [#13899](https://github.com/pnpm/pnpm/issues/13899).
