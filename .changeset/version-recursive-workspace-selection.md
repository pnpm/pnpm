---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed `pnpm version --recursive` so it honors the workspace selection. In recursive mode the version bump now applies to the packages resolved from the workspace filter (`selectedProjectsGraph`), matching the behavior of `pnpm publish --recursive`, instead of always bumping every workspace package [#11348](https://github.com/pnpm/pnpm/issues/11348).
