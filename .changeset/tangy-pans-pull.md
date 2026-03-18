---
"@pnpm/releasing.commands": major
"pnpm": major
"@pnpm/lockfile.make-dedicated-lockfile": minor
"@pnpm/releasing.exportable-manifest": minor
"@pnpm/types": minor
"@pnpm/config.reader": minor
---

`pnpm publish` now works without the `npm` CLI.

The One-time Password feature now reads from `PNPM_CONFIG_OTP` instead of `NPM_CONFIG_OTP`:

```sh
export PNPM_CONFIG_OTP='<your OTP here>'
pnpm publish --no-git-checks
```

If the registry requests OTP and the user has not provided it via the `PNPM_CONFIG_OTP` environment variable or the `--otp` flag, pnpm will prompt the user directly for an OTP code.

If the registry requests web-based authentication, pnpm will print a scannable QR code along with the URL.

Since the new `pnpm publish` no longer calls `npm publish`, some undocumented features may have been unknowingly dropped. If you rely on a feature that is now gone, please open an issue at <https://github.com/pnpm/pnpm/issues>. In the meantime, you can use `pnpm pack && npm publish *.tgz` as a workaround.
