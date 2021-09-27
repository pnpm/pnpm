---
"@pnpm/build-modules": minor
"@pnpm/config": minor
"@pnpm/headless": minor
"@pnpm/hoist": minor
"@pnpm/link-bins": minor
"supi": patch
---

New option added: `extendNodePath`. When it is set to `false`, pnpm does not set the `NODE_PATH` environment variable in the command shims.
