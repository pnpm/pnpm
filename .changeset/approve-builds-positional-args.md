---
"@pnpm/building.commands": minor
"pnpm": minor
---

Allow `pnpm approve-builds` to receive positional arguments for approving or denying packages without the interactive prompt. Prefix a package name with `!` to deny it (e.g. `pnpm approve-builds foo !bar`).
