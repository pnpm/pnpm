---
"@pnpm/config.reader": minor
"@pnpm/installing.commands": minor
"@pnpm/pkg-manifest.utils": minor
"pnpm": minor
---

Added a new setting `runtimeOnFail` that overrides the `onFail` field of `devEngines.runtime` (and `engines.runtime`) in the root project's `package.json`. Accepted values: `ignore`, `warn`, `error`, `download`. For example, setting `runtimeOnFail=download` makes pnpm download the declared runtime version even when the manifest does not set `onFail: "download"`.
