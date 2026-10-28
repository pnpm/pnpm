---
"get-pnpm": patch
---

`npx get-pnpm --no-modify-path`, or `PNPM_NO_MODIFY_PATH=1`, installs pnpm without adding it to PATH. Your shell config file and the Windows registry are left unchanged, and `pnpm setup` prints the lines to add yourself. It needs pnpm 12.16.0 or newer. An older version is refused before anything changes [#17349](https://github.com/pnpm/pnpm/pull/17349).
