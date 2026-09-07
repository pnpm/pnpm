## 1100.1.17

### Patch Changes

- `pnpm unpublish` now completes the two-factor authentication a registry asks for instead of failing with `ERR_PNPM_UNAUTHORIZED` while logged in. A 401 that is an OTP challenge starts the web-based authentication flow, or prompts for a classic one-time password. The obtained password is reused by every request of the run [#14464](https://github.com/pnpm/pnpm/issues/14464).

- Updated dependencies:
  - @pnpm/error@1100.1.4
  - @pnpm/network.fetch@1100.1.15
  - @pnpm/network.web-auth@1101.6.0
