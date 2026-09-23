---
"@pnpm/engine.runtime.node-resolver": patch
"pacquet": patch
"pnpm": patch
---

Node.js runtime resolution now supports Windows ARM64. Node.js 20 and newer resolve native `win-arm64` builds, and older versions fall back to `win-x64` under emulation [#7123](https://github.com/pnpm/pnpm/issues/7123).
