---
"@pnpm/plugin-commands-publishing": minor
"pnpm": minor
---

Handle OTP and webauth authentication flows when publishing packages [#10591](https://github.com/pnpm/pnpm/issues/10591).

When `libnpmpublish` throws an `EOTP` error (requiring two-factor authentication), pnpm now properly handles all three flows:
- **Web auth (authUrl/doneUrl)**: prints the authentication URL with a scannable QR code and polls the `doneUrl` until the authentication is complete, respecting the `Retry-After` header
- **npm-notice flow**: when the error contains `npm-notice` headers with a URL (e.g. for security key authentication), displays the notice message and a QR code for the URL, then prompts for OTP
- **Classic OTP**: prompts the user to enter a one-time password via an interactive prompt
