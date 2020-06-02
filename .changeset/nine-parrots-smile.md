---
"@pnpm/server": patch
---

Downgrade uuid to v3 due to an issue with how pnpm is bundled and published. Due to the flat node_modules structure, when published, all the deps should use the same uuid version. request@2 uses uuid@3
