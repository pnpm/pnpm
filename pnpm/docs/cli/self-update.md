---
id: self-update
title: pnpm self-update
---

Updates pnpm to the latest version or the one specified.

Without a version, pnpm updates to the `latest` dist-tag and does not downgrade. To go back to an older version, pass it explicitly.

```sh
pnpm self-update [<version>]
```

Usage examples:

```sh
pnpm self-update
pnpm self-update 10
pnpm self-update next-10
pnpm self-update 10.6.5
```

## Behavior

The behavior of `pnpm self-update` depends on the project context:

### In a project that pins pnpm

When the project's manifest has a `packageManager` field set to pnpm (or a `devEngines.packageManager` entry for pnpm), `self-update` updates the pinned version in that manifest to the resolved one. In a workspace, this is the workspace root's manifest, even when the command runs in a workspace package. It also installs the resolved version globally, as it does outside a project. The pin is written after the global install, so a failed install leaves the project unchanged.

### Outside a project (or when the pnpm pin is ignored)

If the project does not pin pnpm, or the pin is being ignored via [`pmOnFail: ignore`](../settings/cli.md#pmonfail), `self-update` installs the resolved pnpm version globally and links it to `PNPM_HOME` so it becomes the active pnpm binary on your system.

### When Homebrew or Corepack manages pnpm

If Homebrew installed pnpm or Corepack runs it, `pnpm self-update` leaves the global pnpm alone. In a project that pins pnpm, it still updates the pin. Outside such a project, it fails. Update a Homebrew install with `brew upgrade pnpm` (or `brew upgrade pnpm@<major>` for a versioned formula).

## Project settings are ignored

`pnpm self-update` takes no instruction from the project it is run in:

* pnpm is fetched through the same trusted registry and auth configuration used when switching pnpm versions, so a project's `.npmrc` or `pnpm-workspace.yaml` cannot redirect the download or attach credentials to it, and the project's default `.pnpmfile.(c|m)js` is not loaded. Pnpmfiles from trusted sources (the [`pnpmfile`](../pnpmfile.md#pnpmfile) setting, the global pnpmfile, config dependencies) still apply.
* The project's [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage), [`trustPolicy`](../settings/dependency-resolution.md#trustpolicy), and `ci` settings do not affect `self-update`. They still govern the project's own dependencies; for `self-update`, these values come from the built-in default, your global config, a `PNPM_CONFIG_*` environment variable, or a command-line flag. This stops a repository from either waiving the release-age cooldown or keeping you on an outdated pnpm by raising it, and from weakening the trust check that guards the pnpm download.

When `self-update` refuses a version that is younger than the `minimumReleaseAge` cutoff, an interactive run offers to update anyway; non-interactive runs still fail. CI never prompts, even on a runner that attaches a TTY. Declining the prompt fails with `ERR_PNPM_MINIMUM_RELEASE_AGE_DENIED`. A version that violates the policies fails with `ERR_PNPM_PNPM_RELEASE_POLICY_VIOLATION`. pnpm verifies the identity of the release before installing it. A mismatch fails with `ERR_PNPM_PNPM_ENGINE_IDENTITY_MISMATCH`, and a release pnpm cannot verify fails with `ERR_PNPM_PNPM_ENGINE_IDENTITY_UNVERIFIABLE`.

## What gets installed

For pnpm 12 and newer, `self-update` installs the unscoped `pnpm` package. Its native binaries ship as `@pnpm/exe.<platform>-<arch>` packages, which pnpm links directly, with no Node.js launcher. An older version is installed as `@pnpm/exe` on platforms where it runs, and as `pnpm` otherwise.
