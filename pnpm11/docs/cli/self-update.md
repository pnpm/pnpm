---
id: self-update
title: pnpm self-update
---

Updates pnpm to the latest version or the one specified.

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

If Homebrew installed pnpm or Corepack runs it, `pnpm self-update` leaves the global pnpm alone. In a project that pins pnpm, it still updates the pin. Outside such a project, it fails. Update a Homebrew install with `brew upgrade pnpm` (or `brew upgrade pnpm@11` for a versioned formula).

## Project settings are ignored

Since v11.18.0, `pnpm self-update` takes no instruction from the project it is run in:

* pnpm is fetched through the same trusted registry and auth configuration used when switching pnpm versions, so a project's `.npmrc` or `pnpm-workspace.yaml` cannot redirect the download or attach credentials to it, and the project's default `.pnpmfile.(c|m)js` is not loaded. Pnpmfiles from trusted sources (the [`pnpmfile`](../pnpmfile.md#pnpmfile) setting, the global pnpmfile, config dependencies) still apply.
* The project's [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage), [`trustPolicy`](../settings/dependency-resolution.md#trustpolicy), and `ci` settings do not affect `self-update`. They still govern the project's own dependencies; for `self-update`, these values come from the built-in default, your global config, a `PNPM_CONFIG_*` environment variable, or a command-line flag. This stops a repository from either waiving the release-age cooldown or keeping you on an outdated pnpm by raising it, and from weakening the trust check that guards the pnpm download.

When `self-update` refuses a version that is younger than the `minimumReleaseAge` cutoff, an interactive run offers to update anyway; non-interactive runs still fail. CI never prompts, even on a runner that attaches a TTY.

## Installing pnpm v12 {#installing-pnpm-v12-the-rust-port}

Since v11.10.0, `pnpm self-update` (and `packageManager` version-switching) can install and link **pnpm v12**. It is published under both the `pnpm` and `@pnpm/exe` names on the `latest-12` dist-tag:

```sh
pnpm self-update latest-12
```

v12 ships native binaries as `@pnpm/exe.<platform>-<arch>` packages, which pnpm's built-in installer links directly. There is no Node.js launcher, so the command pays no Node.js startup cost. From v12 onward the install converges on the unscoped `pnpm` package (the Rust executable), even when updating from the SEA `@pnpm/exe` build.
