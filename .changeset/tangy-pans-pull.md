---
"@pnpm/plugin-commands-publishing": major
"pnpm": major
"@pnpm/make-dedicated-lockfile": minor
"@pnpm/exportable-manifest": minor
"@pnpm/types": minor
"@pnpm/config": minor
---

`pnpm publish` now works without the `npm` CLI.

The One-time Password feature now reads from `PNPM_CONFIG_OTP` instead of `NPM_CONFIG_OTP`:

```sh
export PNPM_CONFIG_OTP='<your OTP here>'
pnpm publish --no-git-checks
```

If the registry requests OTP while the user has not provided the OTP via either the `PNPM_CONFIG_OTP` env nor the `--otp` flag, pnpm would prompt the user directly for OTP code.

If the registry requests WebAuth, pnpm would print a scannable QR code along with the URL.

If the registry sends `npm-notice`, pnpm would print scannable QR code for the URLs within it.

Since the new `pnpm publish` no longer calls `npm publish`, some undocumented features may have been unknowingly dropped. If you rely on a feature that is now gone, please open an issue at <https://github.com/pnpm/pnpm/issues>. In the meantime, you can use `pnpm pack && npm publish *.tgz` as a workaround.
