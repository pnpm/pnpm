---
"@pnpm/engine.runtime.commands": patch
"@pnpm/engine.runtime.node-resolver": patch
"@pnpm/global.commands": patch
"@pnpm/global.packages": patch
"@pnpm/hooks.pnpmfile": patch
"pnpm": patch
"pacquet": patch
---

Fixed issues across pnpmfile hooks, Node.js runtime management, and global updates:

- `afterAllResolved` hooks that return `undefined`, `null`, or a non-object value now fail with `ERR_PNPM_BAD_AFTER_ALL_RESOLVED_RETURN`.
- `pnpm env` commands (`use`, `list`, and `remove`) now accept Node.js version specifiers with a leading `v`, including prerelease versions such as `v22.0.0-rc.1`.
- `pnpm update -g --latest` now upgrades global runtimes (`node`, `deno`, and `bun`) and `npm:`-aliased global packages across major versions.
