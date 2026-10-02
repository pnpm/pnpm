---
id: self-update
title: pnpm self-update
---

Updates pnpm to the latest version or the one specified.

```
pnpm self-update [<version>]
```

Usage examples:

```
pnpm self-update
pnpm self-update 10
pnpm self-update next-10
pnpm self-update 10.6.5
```

## Behavior

The behavior of `pnpm self-update` depends on the project context:

### In a project that pins pnpm

When the project's `package.json` has a `packageManager` field set to pnpm (or a `devEngines.packageManager` entry for pnpm), `self-update` only updates the pinned version in `package.json` to the resolved one. It does not install pnpm globally. The next time you run a pnpm command, pnpm will automatically download and switch to the specified version.

### Outside a project (or when the pnpm pin is ignored)

If the project does not pin pnpm, or the pin is being ignored via [`pmOnFail: ignore`](../settings/cli.md#pmonfail), `self-update` installs the resolved pnpm version globally and links it to `PNPM_HOME` so it becomes the active pnpm binary on your system.

## Project settings are ignored

Since v11.18.0, `pnpm self-update` takes no instruction from the project it is run in:

* pnpm is fetched through the same trusted registry and auth configuration used when switching pnpm versions, so a project's `.npmrc` or `pnpm-workspace.yaml` cannot redirect the download or attach credentials to it, and the project's default `.pnpmfile.(c|m)js` is not loaded. Pnpmfiles from trusted sources (the [`pnpmfile`](../pnpmfile.md#pnpmfile) setting, the global pnpmfile, config dependencies) still apply.
* The project's [`minimumReleaseAge`](../settings/dependency-resolution.md#minimumreleaseage), [`trustPolicy`](../settings/dependency-resolution.md#trustpolicy), and `ci` settings do not affect `self-update`. They still govern the project's own dependencies; for `self-update`, these values come from the built-in default, your global config, a `PNPM_CONFIG_*` environment variable, or a command-line flag. This stops a repository from either waiving the release-age cooldown or keeping you on an outdated pnpm by raising it, and from weakening the trust check that guards the pnpm download.

When `self-update` refuses a version that is younger than the `minimumReleaseAge` cutoff, an interactive run offers to update anyway; non-interactive runs still fail. CI never prompts, even on a runner that attaches a TTY.

## Installing pnpm v12

pnpm 12 is the current release line, so a bare `self-update` installs it:

```
pnpm self-update
```

v12 ships native binaries as `@pnpm/exe.<platform>-<arch>` packages, which pnpm's built-in installer links directly. There is no Node.js launcher, so the command pays no Node.js startup cost. From v12 onward the install converges on the unscoped `pnpm` package (the Rust executable), even when updating from the SEA `@pnpm/exe` build.
