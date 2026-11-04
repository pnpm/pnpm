---
"pacquet": minor
---

`pnpm publish` sends release notes to registries declared with `serverType: pnpr`. A package's notes are the section for its version in `CHANGELOG.md`, or the notes that `pnpm version -r` composed from change intents. The new `--release-notes <file>` option gives the notes from a file instead. `--no-release-notes` sends no notes. `--publication-name` names a release published with `--batch`.

In a workspace that uses pnpm's versioning, `pnpm version -r --release-notes <file>` puts a summary above the composed notes of the release.

`pnpm view <pkg>@<version> releaseNotes`, `pnpm outdated --notes`, and `pnpm update --interactive` show the release notes from these registries.
