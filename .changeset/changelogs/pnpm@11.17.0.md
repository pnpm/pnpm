## 11.17.0

### Minor Changes

- Added a new setting, `update.githubActionsServer`, for specifying the base URL of the GitHub server that hosts the repositories of the GitHub Actions referenced by the workflow files (for example, a GitHub Enterprise Server). When the setting is not defined, the URL is read from the `GITHUB_SERVER_URL` environment variable, falling back to `https://github.com`. The URL must use the `https://` or `http://` protocol [#13220](https://github.com/pnpm/pnpm/issues/13220).

  `pnpm outdated` and `pnpm update` no longer fail when the refs of a GitHub Action's repository cannot be read (for example, when the action's repository is private or hosted on a different GitHub server). Such actions are now skipped with a warning.

  Setting `update.githubActions` to `false` now makes `pnpm outdated` and the interactive `pnpm update` skip GitHub Actions dependencies.

### Patch Changes

- The token poll for web-based authentication no longer reads the body of non-OK or still-pending (HTTP 202) responses, and caps the token response body it does read at 64 KiB, so a malicious or compromised registry cannot exhaust memory through the poll [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).

- Fixed `catalog:` references in dependencies and overrides failing to resolve when installing through a pnpr server, which errored with "No catalog entry '<name>' was found for catalog 'default'." even though the catalog entry existed. Also fixed a crash on Windows when installing a nested workspace member (e.g. `packages/foo`) through a pnpr server [#13232](https://github.com/pnpm/pnpm/issues/13232).

- Republished every package: the tarballs published by the v11.13.1 through v11.16.0 releases were missing most of their compiled files due to a packing bug [#13164](https://github.com/pnpm/pnpm/issues/13164).

- Revert script ordering change for `pnpm run --sequential /regex/`

- Support the `from-git` argument in the `pnpm version` command.

- When the authentication URL cannot be rendered as a QR code (for example when it exceeds the maximum QR data capacity), web-based login now displays the URL alone with a warning instead of aborting authentication [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).
