---
"@pnpm/plugin-commands-publishing": patch
"pnpm": patch
---

fix: handle OTP and webauth authentication flows when publishing packages [#10591](https://github.com/pnpm/pnpm/issues/10591)

When `libnpmpublish` throws an `EOTP` error (requiring two-factor authentication), pnpm now properly handles both flows:
- **Classic OTP**: prompts the user to enter a one-time password via an interactive prompt
- **Web auth (modern)**: prints the authentication URL and a scannable QR code for the user, and polls the `doneUrl` until the authentication is complete
