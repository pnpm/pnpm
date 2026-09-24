---
"@pnpm/deps.compliance.license-scanner": patch
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

`pnpm licenses list` now reports the directories where `nodeLinker: hoisted` placed each package, instead of paths under `node_modules/.pnpm` that do not exist in that layout [#8589](https://github.com/pnpm/pnpm/issues/8589).
