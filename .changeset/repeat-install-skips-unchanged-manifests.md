---
"pacquet": patch
---

`pnpm install` in a workspace that is already up to date no longer reads the `package.json` of each project whose file did not change since the last install.
