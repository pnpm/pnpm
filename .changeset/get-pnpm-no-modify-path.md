---
"get-pnpm": patch
---

`npx get-pnpm --no-modify-path`, or `PNPM_NO_MODIFY_PATH=1`, installs pnpm without adding it to PATH. Your shell config file and the Windows registry are left unchanged, and `pnpm setup` prints the lines to add yourself. A pnpm version whose `pnpm setup` lacks that option is refused before anything changes.
