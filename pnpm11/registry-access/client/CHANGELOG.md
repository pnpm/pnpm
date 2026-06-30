# @pnpm/registry-access.client

## 1100.1.5

### Patch Changes

- Updated dependencies [05b95ab]
- Updated dependencies [852d537]
  - @pnpm/network.fetch@1100.1.4
  - @pnpm/error@1100.0.1
  - @pnpm/network.web-auth@1101.1.2

## 1100.1.4

### Patch Changes

- Updated dependencies [a31faa7]
  - @pnpm/network.fetch@1100.1.3
  - @pnpm/network.web-auth@1101.1.1

## 1100.1.3

### Patch Changes

- @pnpm/network.fetch@1100.1.2

## 1100.1.2

### Patch Changes

- @pnpm/network.fetch@1100.1.1

## 1100.1.1

### Patch Changes

- Updated dependencies [60a1eec]
  - @pnpm/network.fetch@1100.1.0

## 1100.1.0

### Minor Changes

- b1fa2d5: Fix `pnpm dist-tag add` and `pnpm dist-tag rm` against npmjs.org failing without `--otp` with `[ERR_PNPM_UNAUTHORIZED] You must be logged in to set dist-tag … "You must provide a one-time pass. Upgrade your client to npm@latest in order to use 2FA."`. pnpm now sends `npm-auth-type: web` on dist-tag writes and surfaces the resulting OTP challenge through the existing browser-based 2FA flow (the same `withOtpHandling` helper used by `pnpm publish`), so the browser opens, the user authenticates, and the dist-tag is set on retry. `--otp=<code>` continues to work via the classic flow.

### Patch Changes

- Updated dependencies [b1fa2d5]
- Updated dependencies [2cadfb5]
  - @pnpm/network.fetch@1100.0.8
  - @pnpm/network.web-auth@1101.1.0

## 1100.0.1

### Patch Changes

- @pnpm/network.fetch@1100.0.7
