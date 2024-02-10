---
"@pnpm/cli-utils": major
"pnpm": major
"@pnpm/exportable-manifest": major
---

pnpm will now check the `package.json` file for a `packageManager` field. If this field is present and specifies a different package manager or a different version of pnpm than the one you're currently using, pnpm will not proceed. This ensures that you're always using the correct package manager and version that the project requires.

To disable this behaviour, set the `package-manager-strict` setting to `false` or the `COREPACK_ENABLE_STRICT` env variable to `0`.
