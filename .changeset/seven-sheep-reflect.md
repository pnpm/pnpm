---
"@pnpm/package-is-installable": patch
"@pnpm/env.system-node-version": patch
"pnpm": patch
---

Don't crash if the `use-node-version` setting is used and the system has no Node.js installed [#8769](https://github.com/pnpm/pnpm/issues/8769).
