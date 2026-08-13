---
"@pnpm/crypto.shasums-file": minor
"@pnpm/engine.runtime.node-resolver": minor
"@pnpm/resolving.default-resolver": patch
"pnpm": minor
"pacquet": minor
---

Resolving a Node.js runtime version (`devEngines.runtime` / `runtime:` specifiers) is now much faster: the per-version release metadata is cached in the pnpm cache directory after its signature is verified, and an exact stable version such as `runtime:22.23.2` no longer downloads the Node.js release index. A pinned runtime whose metadata was fetched once resolves without any network access, which removes the noticeable delay on the first `node` invocation in a project pinning an already-downloaded runtime [#13899](https://github.com/pnpm/pnpm/issues/13899).
