---
"@pnpm/deps.compliance.license-scanner": patch
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

With `nodeLinker: hoisted`, `pnpm licenses list` reported paths under `node_modules/.pnpm` that do not exist. It now reports the directory where the hoisted linker placed each package [#8589](https://github.com/pnpm/pnpm/issues/8589).
