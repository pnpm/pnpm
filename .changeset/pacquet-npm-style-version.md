---
"pacquet": minor
---

`pnpm version` now supports the npm-style bump forms: `pnpm version <major|minor|patch|premajor|preminor|prepatch|prerelease>` and `pnpm version <exact-version>` (also recursively with `-r`), with `--preid`, `--allow-same-version`, `--message`, `--no-git-tag-version`, `--no-commit-hooks`, `--sign-git-tag`, `--tag-version-prefix`, and `--json`. The bump runs the `preversion`/`version`/`postversion` lifecycle scripts and records the new version as a git commit and tag.
