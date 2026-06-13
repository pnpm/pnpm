---
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fix `pnpm update --prod` installing devDependencies when they are not present in node_modules. The `--prod`, `--dev`, and `--no-optional` flags now correctly control which dependency types are installed during an update, consistent with `pnpm install` behavior.
