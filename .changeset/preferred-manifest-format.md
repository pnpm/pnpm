---
"@pnpm/types": minor
"@pnpm/workspace.project-manifest-reader": minor
"@pnpm/workspace.projects-reader": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Added a new `preferredManifestFormat` setting in `pnpm-workspace.yaml` that selects which manifest format pnpm should read and write when multiple manifest files coexist in the same directory (e.g. `package.json` and `package.json5`). Allowed values are `json` (default), `json5`, and `yaml`. Falls back to the default chain (`json` > `json5` > `yaml`) when the preferred format is missing. The setting only applies to manifests within the workspace, not to dependencies. Also fixes a workspace-discovery bug where a directory containing multiple manifest files would produce duplicate project entries [#3027](https://github.com/pnpm/pnpm/issues/3027) [#5541](https://github.com/pnpm/pnpm/issues/5541).
