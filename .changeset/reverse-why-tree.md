---
"@pnpm/reviewing.dependencies-hierarchy": minor
"@pnpm/list": minor
"@pnpm/plugin-commands-listing": minor
"pnpm": minor
---

`pnpm why` now shows a reverse dependency tree. The searched package appears at the root with its dependents as branches, walking back to workspace roots. This replaces the previous forward-tree output which was noisy and hard to read for deeply nested dependencies.
