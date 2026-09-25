---
"@pnpm/network.web-auth": minor
"@pnpm/auth.commands": patch
"pnpm": patch
"pacquet": patch
---

When the authentication URL cannot be rendered as a QR code (for example when it exceeds the maximum QR data capacity), web-based login now displays the URL alone with a warning instead of aborting authentication [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).
