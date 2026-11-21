---
"get-pnpm": patch
---

`npx get-pnpm` now installs pnpm from a registry that carries no npm signatures, such as Azure Artifacts, by checking the download against the signatures on registry.npmjs.org. It failed with "carries no npm registry signature".
