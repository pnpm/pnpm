---
"@pnpm/releasing.commands": minor
"pnpm": minor
---

Fixed and expanded `pnpm version` to match npm behavior:

- Accept an explicit semver version (e.g. `pnpm version 1.2.3`) in addition to bump types.
- Recognize `--no-commit-hooks`, `--no-git-tag-version`, `--sign-git-tag`, and `--message`.
- Fix `--no-git-checks` which was previously parsed incorrectly.
- Create a git commit and annotated tag for the version bump when running inside a git repository (unless `--no-git-tag-version` is used). `--message` supports `%s` replacement with the new version, and `--tag-version-prefix` controls the tag prefix (defaults to `v`). Git commits and tags are always skipped in recursive mode since multiple packages may be bumped to different versions in a single run [#11271](https://github.com/pnpm/pnpm/issues/11271).
