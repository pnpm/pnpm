---
"@pnpm/pnpr": minor
---

A hosted registry rejects a publish whose version metadata disagrees with the `package.json` inside its archive. The `400` response names the package, the version, and each differing field. The checked fields are the dependencies, the platform fields, `bin`, and the install scripts. A publish whose archive does not decode, or holds more than one `package.json`, is rejected too. Rewrites that npm and older clients make while publishing are accepted, such as a `bin` string expanded into an object. A version is published without semver build metadata. Its metadata is served as its archive states it, with `hasInstallScript` and `_hasShrinkwrap` always set.
