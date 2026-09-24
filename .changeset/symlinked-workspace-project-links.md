---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.read-projects-context": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm install` for workspace projects reached through a symlink, such as a `packages` directory that links to a folder outside the workspace. pnpm now installs their dependencies, and the links in their `node_modules` resolve [#1044](https://github.com/pnpm/pnpm/issues/1044).
