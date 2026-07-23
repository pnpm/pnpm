---
"@pnpm/config.reader": minor
"@pnpm/engine.pm.commands": minor
"@pnpm/installing.client": minor
"pnpm": minor
---

`pnpm self-update` no longer lets a repository weaken the protections around the pnpm download:

- pnpm is now fetched through the same trusted registry/auth configuration used when switching pnpm versions, so a project `.npmrc` or `pnpm-workspace.yaml` cannot redirect the download or attach credentials to it, and the project's default `.pnpmfile.(c|m)js` is no longer loaded. Pnpmfiles from trusted sources (the `pnpmfile` setting, the global pnpmfile, config-dependency plugins) still apply.
- A project `pnpm-workspace.yaml` may only *tighten* the `minimumReleaseAge` policy that governs `self-update`: it can raise the cutoff or turn `minimumReleaseAgeStrict` on, but it can no longer lower the cutoff, turn strict mode off, or exempt pnpm through `minimumReleaseAgeExclude`. A value set from a trusted source (global config, environment variable, CLI flag) wins over the project's.

When `self-update` refuses a version that is younger than the cutoff, the error now names where the cutoff came from, and an interactive run offers to update anyway.
