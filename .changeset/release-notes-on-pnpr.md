---
"pacquet": minor
---

`pnpm publish` sends release notes to registries declared with `serverType: pnpr`. A package's notes are its `CHANGELOG.md` section for the version, the notes composed from change intents by `pnpm version -r`, or the text given with the new `--release-notes` option. `--no-release-notes` sends none, and `--publication-name` names a release published with `--batch`. In a workspace that uses pnpm's versioning, `pnpm version -r --release-notes` adds a summary above the composed notes of the release.

`pnpm view <pkg>@<version> releaseNotes`, `pnpm outdated --notes`, and `pnpm update --interactive` show the release notes from these registries.
