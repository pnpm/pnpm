---
"@pnpm/pkg-manifest.utils": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/deps.inspection.list": patch
"pnpm": patch
---

Address CodeQL static-analysis findings: guard manifest dependency writes against prototype-polluting keys (`__proto__`, `constructor`, `prototype`), and replace a potentially super-linear semver-detection regex in registry 404 hints with an O(n) parser.
