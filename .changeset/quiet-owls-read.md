---
"pacquet": minor
---

The global `node` shim created by pnpm now uses the Node.js version from the nearest `.node-version` file when the project declares no Node.js runtime in `package.json`. A `.node-version` file takes precedence over an `.nvmrc` file in the same directory.
