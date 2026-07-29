## 1100.3.0

### Minor Changes

- `pnpm self-update` no longer takes any instruction from the project it is run in:

  - pnpm is fetched through the same trusted registry and auth configuration used when switching pnpm versions, so a project `.npmrc` or `pnpm-workspace.yaml` can no longer redirect the download or attach credentials to it, and the project's default `.pnpmfile.(c|m)js` is no longer loaded. Pnpmfiles from trusted sources (the `pnpmfile` setting, the global pnpmfile, config dependencies) still apply.
  - The `minimumReleaseAge` settings in `pnpm-workspace.yaml` no longer affect `self-update`. They still govern the project's own dependencies; for `self-update` the cooldown now comes from the built-in default, your global config, a `PNPM_CONFIG_*` environment variable, or a command-line flag. This fixes `self-update` failing inside a workspace that raises the cutoff while succeeding everywhere else, and stops a repository from either waiving the cooldown or keeping you on an outdated pnpm by raising it.
  - The same applies to the `trustPolicy` settings and to `ci`: a project can no longer weaken the trust check that guards the pnpm download, nor re-enable the confirmation prompt that a CI run suppresses.

  When `self-update` refuses a version that is younger than the cutoff, an interactive run now offers to update anyway; non-interactive runs still fail. CI never prompts, even on a runner that attaches a TTY.

### Patch Changes

- Updated dependencies:
  - @pnpm/engine.runtime.node-resolver@1101.1.19
  - @pnpm/fetching.binary-fetcher@1102.0.7
  - @pnpm/fetching.directory-fetcher@1100.0.26
  - @pnpm/fetching.git-fetcher@1102.0.10
  - @pnpm/fetching.tarball-fetcher@1102.0.10
  - @pnpm/hooks.types@1100.2.4
  - @pnpm/network.auth-header@1101.1.7
  - @pnpm/network.fetch@1100.1.9
  - @pnpm/resolving.default-resolver@1100.3.21
  - @pnpm/resolving.npm-resolver@1102.1.9
  - @pnpm/resolving.resolver-base@1100.5.5
  - @pnpm/types@1101.7.0
