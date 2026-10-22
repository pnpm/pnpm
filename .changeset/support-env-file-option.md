---
"pacquet": minor
---

Added a repeatable global `--env-file <path>` flag to load environment variables from dotenv-style files before running a command. Later files override earlier files, variables already in the environment are preserved, and loaded variables are visible to user-level `.npmrc` interpolation and lifecycle scripts [#7111](https://github.com/pnpm/pnpm/issues/7111).
