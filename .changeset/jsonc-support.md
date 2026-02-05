---
"@pnpm/constants": minor
"@pnpm/read-project-manifest": minor
"@pnpm/write-project-manifest": minor
"@pnpm/find-packages": minor
"@pnpm/plugin-commands-publishing": minor
"pnpm": minor
---

Add support for `package.jsonc` manifest files.

pnpm now recognizes `package.jsonc` as a valid manifest file name. JSONC (JSON with Comments) is a subset of JSON5 that allows comments and trailing commas while maintaining strict JSON syntax for data structures.

The order of precedence when searching for a manifest file is:
1. `package.json`
2. `package.json5`
3. `package.jsonc`
4. `package.yaml`
