---
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
"pacquet": patch
---

The install summary now names the version each dependency resolved to when `node-linker` is `hoisted`. It reports dependencies restored after `node_modules` is deleted. It reports unsupported optional dependencies removed from `node_modules`. It shows both sides of a version change. The summary showed the range recorded in `package.json`, or nothing at all [#15161](https://github.com/pnpm/pnpm/issues/15161).
