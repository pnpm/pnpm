---
"@pnpm/plugin-commands-publishing": patch
"pnpm": patch
---

fix: handle OTP and webauth authentication flows when publishing packages [#10591](https://github.com/pnpm/pnpm/issues/10591)

When `libnpmpublish` throws an `EOTP` error (requiring two-factor authentication), pnpm now properly handles all flows:
- **Classic OTP**: prompts the user to enter a one-time password via an interactive prompt
- **Web auth (legacy)**: when the error body contains `authUrl` and `doneUrl`, prints the authentication URL and a scannable QR code for the user, and polls the `doneUrl` until the authentication is complete
- **Web auth (npm-notice)**: when the error headers contain an `npm-notice` message with a URL (e.g. security key / passkey authentication), extracts the URL, displays the notice message with a scannable QR code, derives the polling URL from the registry and token, and polls until authentication is complete
