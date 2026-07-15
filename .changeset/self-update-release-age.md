---
"@pnpm/config.reader": minor
"pnpm": minor
---

`pnpm self-update` now loads config from trusted sources only: the project `pnpm-workspace.yaml` settings, the project `.npmrc`, and the project's default `.pnpmfile.(c|m)js` no longer steer it, so a repo-controlled workspace cannot change the registry, auth, hooks, or release-age policy that `self-update` resolves pnpm with. `self-update` still reads the project `package.json` to detect and bump a `packageManager` / `devEngines.packageManager` pin. `minimumReleaseAgeStrict` defaults to `true` for `self-update` so a freshly published pnpm is refused until the cutoff elapses; disable the cutoff via `minimumReleaseAge: 0`, or strict mode via `minimumReleaseAgeStrict: false` (global config, CLI flag, or env).
