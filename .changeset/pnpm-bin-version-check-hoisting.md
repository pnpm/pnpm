---
"pnpm": patch
---

Fixed the pnpm CLI crashing with a confusing `SyntaxError: Invalid regular expression flags` instead of printing a clear "requires Node.js v22.13" error when launched on an unsupported Node.js version. The Node.js version check in `bin/pnpm.mjs` was effectively dead code because the static `import` of the bundled `dist/pnpm.mjs` was hoisted by the ES module loader and parsed before the check could run [#11546](https://github.com/pnpm/pnpm/issues/11546).
