---
"@pnpm/resolving.resolver-base": patch
"pacquet": patch
"pnpm": patch
---

A runtime installed through `devEngines.runtime` now matches the host when `supportedArchitectures` lists several platforms. Listing `os: [darwin, linux]` and `cpu: [x64, arm64]` used to install the runtime built for the first entry of each list, so a machine running Linux on arm64 got a macOS x64 Node.js that could not execute [#13898](https://github.com/pnpm/pnpm/issues/13898).
