---
"@pnpm/link-bins": patch
"pnpm": patch
---

Fixed "input line too long" error on Windows when running lifecycle scripts with the global virtual store enabled. The `NODE_PATH` in command shims no longer includes all paths from `Module._nodeModulePaths()`. Instead, it includes only the package's own dependencies directory (e.g., `.pnpm/pkg@version/node_modules`) and the hoisted `node_modules` directory. The package-level path is needed so that tools like `import-local` (used by jest, eslint, etc.) which resolve from CWD can find the correct dependency versions.
