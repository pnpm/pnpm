## 12.0.0-alpha.19

### Minor Changes

- Added a new setting, `update.githubActionsServer`, for specifying the base URL of the GitHub server that hosts the repositories of the GitHub Actions referenced by the workflow files (for example, a GitHub Enterprise Server). When the setting is not defined, the URL is read from the `GITHUB_SERVER_URL` environment variable, falling back to `https://github.com`. The URL must use the `https://` or `http://` protocol [#13220](https://github.com/pnpm/pnpm/issues/13220).

  `pnpm outdated` and `pnpm update` no longer fail when the refs of a GitHub Action's repository cannot be read (for example, when the action's repository is private or hosted on a different GitHub server). Such actions are now skipped with a warning.

  Setting `update.githubActions` to `false` now makes `pnpm outdated` and the interactive `pnpm update` skip GitHub Actions dependencies.

- Added the `pnpm unpublish` command: remove a package from the registry entirely (requires `--force`), or remove the versions matching `<package>@<range>`, re-pointing `dist-tags` that referenced them and deleting the orphaned tarballs. Supports `--registry` and `--otp`.

### Patch Changes

- The token poll for web-based authentication no longer reads the body of non-OK or still-pending (HTTP 202) responses, and caps the token response body it does read at 64 KiB, so a malicious or compromised registry cannot exhaust memory through the poll [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).

- `pnpm install --no-runtime` now works without `--frozen-lockfile`: on a fresh install, runtime dependencies are resolved and recorded in the lockfile, but their archives are not downloaded and their bins are not linked.

- `pnpm outdated` now aligns its table borders when the output is colorized. The color escape codes in the `Package` and `Latest` cells were being counted as visible characters, so the columns and box-drawing borders drifted out of alignment on a terminal.

- `pnpm pack` and `pnpm publish` no longer let workspace-root `.gitignore` / `.npmignore` rules exclude files matched by the package manifest's `files` allowlist. Workspace packages whose build output is gitignored at the workspace root (for example a compiled `lib/` directory listed in `files`) were published with almost all payload files missing [#13164](https://github.com/pnpm/pnpm/issues/13164).

- Fixed `pnpm update --latest` failing with `ERR_PNPM_PACKAGE_MANAGER_UPDATE_RESOLVE_LATEST` when a dependency uses the `workspace:` (or `link:` / `file:`) protocol. Such a dependency links a local package that may not be published, so there is no registry "latest" to resolve — it is now skipped and preserved verbatim, matching the TypeScript CLI. Previously only `workspace:<path>` specifiers were skipped, so `workspace:*` / `workspace:^1.0.0` deps pointing at unpublished packages made `--latest` try to fetch them from the registry and 404.

- `pnpm view` now accepts the `--registry` option, matching the TypeScript CLI. Previously the flag was rejected as an unknown argument.

- When the authentication URL cannot be rendered as a QR code (for example when it exceeds the maximum QR data capacity), web-based login now displays the URL alone with a warning instead of aborting authentication [pnpm/pnpm#12721](https://github.com/pnpm/pnpm/issues/12721).
