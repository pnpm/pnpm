---
"@pnpm/deps.compliance.license-scanner": patch
"pnpm": patch
"pacquet": patch
---

`pnpm licenses list` now reports the directories where `nodeLinker: hoisted` placed each package, instead of paths under `node_modules/.pnpm` that do not exist in that layout. pnpm v12 also reads each package's license from there, so these packages are no longer reported as `Unknown` [#8589](https://github.com/pnpm/pnpm/issues/8589).
