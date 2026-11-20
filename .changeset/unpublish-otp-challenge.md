---
"@pnpm/network.web-auth": minor
"@pnpm/registry-access.client": patch
"@pnpm/registry-access.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm unpublish` now completes the two-factor authentication a registry asks for instead of failing with `ERR_PNPM_UNAUTHORIZED` while logged in. A 401 that is an OTP challenge starts the web-based authentication flow, or prompts for a classic one-time password. The obtained password is reused by every request of the run [#14464](https://github.com/pnpm/pnpm/issues/14464).
