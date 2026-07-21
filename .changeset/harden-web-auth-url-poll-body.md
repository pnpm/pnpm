---
"@pnpm/network.web-auth": minor
"@pnpm/auth.commands": patch
"pnpm": patch
"pacquet": patch
---

Hardened the web-based authentication flow against a malicious or compromised registry: the token poll no longer reads the body of non-OK or still-pending (HTTP 202) responses and caps the token response body it does read at 64 KiB, and a QR code that cannot be generated (for example when the authentication URL exceeds the maximum QR data capacity) now falls back to displaying the URL alone with a warning instead of aborting authentication [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).
