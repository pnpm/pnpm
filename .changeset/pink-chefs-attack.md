---
"@pnpm/core": minor
"pnpm": minor
---

Added support for a new lifecycle script: `pnpm:devPreinstall`. This script works only in the root `package.json` file, only during local development, and runs before installation happens.
