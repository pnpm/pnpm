---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

`pnpm audit --fix=override` now respects `saveExact` and `savePrefix` config. Previously, override versions always used a caret prefix (`^`) regardless of these settings, while `--fix=update` correctly inherited them through the normal install pipeline. See pnpm/pnpm#13209.
