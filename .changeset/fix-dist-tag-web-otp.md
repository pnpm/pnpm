---
"@pnpm/registry-access.client": minor
"@pnpm/registry-access.commands": patch
"@pnpm/network.fetch": patch
"pnpm": patch
---

Fix `pnpm dist-tag add` and `pnpm dist-tag rm` against npmjs.org failing without `--otp` with `[ERR_PNPM_UNAUTHORIZED] You must be logged in to set dist-tag … "You must provide a one-time pass. Upgrade your client to npm@latest in order to use 2FA."`. pnpm now sends `npm-auth-type: web` on dist-tag writes and surfaces the resulting OTP challenge through the existing browser-based 2FA flow (the same `withOtpHandling` helper used by `pnpm publish`), so the browser opens, the user authenticates, and the dist-tag is set on retry. `--otp=<code>` continues to work via the classic flow.
