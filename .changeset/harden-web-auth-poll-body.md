---
"@pnpm/network.web-auth": minor
"pnpm": patch
"pacquet": patch
---

The token poll for web-based authentication no longer reads the body of non-OK or still-pending (HTTP 202) responses, and caps the token response body it does read at 64 KiB, so a malicious or compromised registry cannot exhaust memory through the poll [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).
