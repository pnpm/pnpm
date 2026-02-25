---
"@pnpm/link-bins": patch
"pnpm": patch
---

Fixed "input line too long" error on Windows when running lifecycle scripts with the global virtual store enabled. The `NODE_PATH` in command shims no longer includes all paths from `Module._nodeModulePaths()`. Instead, it includes only the package's bundled dependencies directory (e.g., `.pnpm/pkg@version/node_modules/pkg/node_modules`), the package's sibling dependencies directory (e.g., `.pnpm/pkg@version/node_modules`), and the hoisted `node_modules` directory. These paths are needed so that tools like `import-local` (used by jest, eslint, etc.) which resolve from CWD can find the correct dependency versions [#10673](https://github.com/pnpm/pnpm/pull/10673).
