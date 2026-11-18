---
"@pnpm/cli.parse-cli-args": patch
"pnpm": patch
---

An unknown option passed via a shorthand is now reported as typed, and the shorthand of the other case is suggested when the command supports it. For instance, `pnpm update -l` used to fail with `Unknown option: 'long'`, as `-l` is short for `--long`. Now it fails with `Unknown option: '-l'` and suggests `-L` (`--latest`).
