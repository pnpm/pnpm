---
"@pnpm/headless": major
"@pnpm/hoist": major
"@pnpm/modules-cleaner": major
"@pnpm/plugin-commands-rebuild": major
"@pnpm/plugin-commands-store": major
"pnpm": major
"@pnpm/resolve-dependencies": major
"supi": minor
---

The structure of virtual store directory changed. No subdirectory created with the registry name.
So instead of storing packages inside `node_modules/.pnpm/<registry>/<pkg>`, packages are stored
inside `node_modules/.pnpm/<pkg>`.
