---
"@pnpm/engine.pm.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm setup` no longer makes Node.js print a `MODULE_TYPELESS_PACKAGE_JSON` warning about `dist/worker.js` on every command. The `package.json` it writes next to a standalone executable now declares `"type": "module"`.
