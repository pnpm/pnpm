---
"@pnpm/auth.commands": minor
"@pnpm/building.commands": minor
"@pnpm/deps.compliance.commands": minor
"@pnpm/exec.commands": minor
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/network.web-auth": minor
"@pnpm/patching.commands": minor
"@pnpm/registry-access.commands": minor
"@pnpm/releasing.commands": minor
"pnpm": minor
---

Replaced `enquirer` with `@inquirer/prompts` for all interactive prompts. Fixes the `update -i` scrolling overflow bug where long choice lists were clipped in the terminal [#6643](https://github.com/pnpm/pnpm/issues/6643).

**User-facing changes:**
- `pnpm update -i` / `pnpm update -i --latest`: Scrolling now works correctly when many packages are available; the new library uses visual-line-aware pagination via `usePagination`
- `pnpm audit --fix -i`: Same scrolling fix for vulnerability selection
- `pnpm approve-builds`: Interactive build approval prompts updated
- `pnpm patch`: Version selection and "apply to all" prompts updated
- `pnpm patch-remove`: Patch removal selection updated
- `pnpm publish`: Branch confirmation prompt updated
- `pnpm login`: Credential prompts updated
- `pnpm run` / `pnpm exec` (with `verifyDepsBeforeRun=prompt`): Confirmation prompt updated

Vim-style `j`/`k` keys still work for up/down navigation in all interactive prompts.

**Internal:** The `OtpEnquirer` and `LoginEnquirer` DI interfaces changed from `{ prompt }` to `{ input }` / `{ input, password }` respectively. Plugins or custom builds that inject their own enquirer mock will need to update.
