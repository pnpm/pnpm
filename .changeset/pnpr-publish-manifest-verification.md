---
"@pnpm/pnpr": minor
---

A hosted pnpr registry rejects a publish whose version metadata disagrees with the `package.json` inside its archive. Its `400` response names the package, the version, and each differing field. pnpr checks the dependencies, the platform fields, `bin`, and the install scripts. pnpr also rejects a publish whose archive does not decode or holds more than one `package.json`. pnpr accepts the rewrites that npm and older clients make while publishing, such as a `bin` string expanded into an object. pnpr publishes a version without semver build metadata. pnpr serves a version's checked fields as its archive states them, with `hasInstallScript` and `_hasShrinkwrap` always set. Other fields, such as `description`, are served as submitted.
