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

Since the new `pnpm publish` no longer calls `npm publish`, some undocumented features may have been unknowingly dropped. If you rely on a feature that is now gone, please open an issue at <https://github.com/pnpm/pnpm/issues>. In the meantime, you can use `pnpm pack && npm publish *.tgz` as a workaround.
