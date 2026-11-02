---
"@pnpm/pnpr": minor
---

A hosted registry now verifies each published version's metadata against the `package.json` inside its archive. A publish whose dependencies, platform fields, `bin`, or install scripts disagree with the archive is rejected with a 400 that names the differing fields, as is an archive that does not decode or names its manifest ambiguously. What npm and older publish clients rewrite on the way, such as a `bin` string expanded into an object or optional dependencies copied into `dependencies`, is not a disagreement. The version is then served from what its archive says, with `hasInstallScript` and `_hasShrinkwrap` always set. `pnpr verify` reports hosted versions published earlier whose metadata disagrees with their archive.
