---
"@pnpm/pnpr": minor
---

pnpr accepts a version published with platform variants only, without an archive for every other host. It generates the archive that npm's documents point to, whose `package.json` lists the variants' platforms, so npm reports an unsupported platform instead of installing an empty package.
